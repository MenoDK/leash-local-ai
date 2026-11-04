//! Everything the handlers share, assembled once at start-up.
//!
//! The demo is wired from the real libraries: nothing here is a mock of them. The only stand-ins are the two language
//! models (deterministic, no text generation) and, in the broker, three harmless system programs bound as commands.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use leash_broker::{
    Approval, ArgKind, ArgSpec, Broker, BrokerConfig, Ledger, LedgerKey, Limits, Manifest,
    sha256_of,
};
use leash_gateway::{Gateway, Locality, OllamaProvider, StandIn};
use leash_memory::{GuardConfig, HashEmbedder, Memory, MemoryConfig};
use tokio::sync::RwLock;

use crate::bridge::Bridge;
use crate::config::Config;

pub type Shared = Arc<AppState>;

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("{what}: {reason}")]
    Setup { what: &'static str, reason: String },
}

fn setup(what: &'static str) -> impl Fn(&dyn std::fmt::Display) -> StartError {
    move |reason| StartError::Setup {
        what,
        reason: reason.to_string(),
    }
}

/// The copy of a program that the `scratch` command is bound to, kept so the lab can tamper with it and put it back.
pub struct Scratch {
    pub path: PathBuf,
    original: Vec<u8>,
}

impl std::fmt::Debug for Scratch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Scratch(..)")
    }
}

impl Scratch {
    fn replace(&self, content: &[u8]) -> std::io::Result<()> {
        // A new file renamed over the old one: never a write into an inode that something might be executing.
        let staging = self.path.with_extension("staging");
        std::fs::write(&staging, content)?;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&staging, &self.path)
    }

    /// Change the binary by one byte, as an attacker with write access to the directory would.
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn tamper(&self) -> std::io::Result<()> {
        let mut changed = self.original.clone();
        changed.push(0);
        self.replace(&changed)
    }

    /// Put the original bytes back.
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn restore(&self) -> std::io::Result<()> {
        self.replace(&self.original)
    }
}

pub struct AppState {
    pub config: Config,
    pub started: Instant,
    pub gateway: Gateway,
    pub local: Arc<StandIn>,
    pub remote: Arc<StandIn>,
    pub memory: RwLock<Memory>,
    pub memory_config: MemoryConfig,
    pub guard: GuardConfig,
    /// The runtime copy of the corpus. The server only ever reads inside it.
    pub corpus: PathBuf,
    pub broker: Broker,
    pub ledger_key: Option<Vec<u8>>,
    pub scratch: Scratch,
    pub work_dir: PathBuf,
    pub bridge: Bridge,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppState(..)")
    }
}

/// Copy a directory tree of regular files. Symbolic links are not followed or copied.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn random_hex(bytes: usize) -> Result<String, StartError> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|e| StartError::Setup {
        what: "random bytes",
        reason: e.to_string(),
    })?;
    Ok(hex::encode(buffer))
}

/// A note that leaks credentials, built at start so that no secret-shaped string exists in the repository.
fn write_leaky_note(docs: &Path) -> Result<(), StartError> {
    let provider_key = format!("{}-{}-{}", "sk", "live", random_hex(20)?);
    let password = random_hex(10)?;
    let bearer = random_hex(16)?;
    let note = format!(
        "# Deploy notes\n\n## Staging release\n\nThe staging release is deployed from the build machine. Somebody pasted the \
         environment of the last run into these notes, which is exactly the kind of mistake the memory must survive.\n\n\
         PROVIDER_API_KEY={provider_key}\nDB_PASSWORD={password}\nAuthorization: Bearer {bearer}\n\n\
         After the release, check the dashboard and write the version in the change log.\n"
    );
    std::fs::write(docs.join("deploy-notes.md"), note).map_err(|e| setup("deploy notes")(&e))
}

fn bind_copy(dir: &Path, program: &str, name: &str) -> Result<(String, String), StartError> {
    let source = ["/usr/bin", "/bin"]
        .iter()
        .map(|d| Path::new(d).join(program))
        .find(|p| p.is_file())
        .ok_or_else(|| StartError::Setup {
            what: "command binding",
            reason: format!("{program} was not found in /usr/bin or /bin"),
        })?;
    let target = dir.join(name);
    std::fs::copy(&source, &target).map_err(|e| setup("command binding")(&e))?;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| setup("command binding")(&e))?;
    let hash = sha256_of(&target).map_err(|e| setup("command binding")(&e))?;
    Ok((target.display().to_string(), hash))
}

fn manifest(
    id: &str,
    description: &str,
    binary: &str,
    hash: &str,
    args: Vec<ArgSpec>,
    flags: &[&str],
    approval: Approval,
) -> Manifest {
    Manifest {
        id: id.to_owned(),
        version: "1".to_owned(),
        description: description.to_owned(),
        binary: binary.to_owned(),
        binary_sha256: hash.to_owned(),
        args,
        allowed_flags: flags.iter().map(|f| (*f).to_owned()).collect(),
        timeout_seconds: 5,
        limits: Limits::default(),
        approval,
        // `echo` prints a `--` it is given, so the commands built on it do not get one. Positional arguments still
        // cannot start with `-`, which is the rule that matters.
        end_of_options: false,
    }
}

fn demo_manifests(bin: &Path) -> Result<(Vec<Manifest>, PathBuf), StartError> {
    let (say, say_hash) = bind_copy(bin, "echo", "say")?;
    let (kernel, kernel_hash) = bind_copy(bin, "uname", "kernel")?;
    let (when, when_hash) = bind_copy(bin, "date", "when")?;
    let (scratch, scratch_hash) = bind_copy(bin, "echo", "scratch")?;
    let word = |name: &str| ArgSpec {
        name: name.to_owned(),
        kind: ArgKind::Token,
        required: true,
        position: 0,
    };
    let format = ArgSpec {
        name: "format".to_owned(),
        kind: ArgKind::Enum {
            values: vec!["+%Y-%m-%d".to_owned(), "+%s".to_owned()],
        },
        required: true,
        position: 0,
    };
    let manifests = vec![
        manifest(
            "say",
            "Print one word. A person approves every run.",
            &say,
            &say_hash,
            vec![word("word")],
            &[],
            Approval::Always,
        ),
        manifest(
            "kernel",
            "Print the kernel name. Harmless and read-only, so it runs without asking.",
            &kernel,
            &kernel_hash,
            vec![],
            &["-s"],
            Approval::Never,
        ),
        manifest(
            "when",
            "Print the date in one of two fixed formats. Harmless and read-only.",
            &when,
            &when_hash,
            vec![format],
            &[],
            Approval::Never,
        ),
        manifest(
            "scratch",
            "A command whose binary the lab modifies, to show that a changed file is refused.",
            &scratch,
            &scratch_hash,
            vec![word("word")],
            &[],
            Approval::Always,
        ),
    ];
    Ok((manifests, PathBuf::from(scratch)))
}

impl AppState {
    /// Build every component from the real libraries and index the demo corpus.
    ///
    /// # Errors
    /// A directory cannot be prepared, the ledger does not verify, or a provider is misconfigured.
    pub async fn build(config: Config) -> Result<Self, StartError> {
        std::fs::create_dir_all(&config.data_dir).map_err(|e| setup("data directory")(&e))?;
        let data = config
            .data_dir
            .canonicalize()
            .map_err(|e| setup("data directory")(&e))?;
        let bin = data.join("bin");
        let work_dir = data.join("work");
        for dir in [&bin, &work_dir] {
            std::fs::create_dir_all(dir).map_err(|e| setup("data directory")(&e))?;
        }

        // Gateway: a local model first, then a remote one. Text marked confidential can only reach the local ones.
        let local = Arc::new(StandIn::new("local-stand-in", Locality::Local));
        let remote = Arc::new(StandIn::new("remote-stand-in", Locality::Remote));
        let mut gateway = Gateway::new(3, Duration::from_secs(20));
        if let Some(url) = &config.ollama_url {
            let ollama = OllamaProvider::new("ollama", url, &config.ollama_model)
                .map_err(|e| setup("LEASH_OLLAMA_URL")(&e))?;
            gateway.register(Arc::new(ollama));
        }
        gateway.register(local.clone()).register(remote.clone());

        // Memory: a runtime copy of the corpus plus two files that only exist while the server runs.
        let corpus = data.join("corpus");
        if corpus.exists() {
            std::fs::remove_dir_all(&corpus).map_err(|e| setup("corpus")(&e))?;
        }
        copy_tree(&config.corpus_dir, &corpus).map_err(|e| StartError::Setup {
            what: "corpus",
            reason: format!("{}: {e}", config.corpus_dir.display()),
        })?;
        let docs = corpus.join("docs");
        std::fs::create_dir_all(&docs).map_err(|e| setup("corpus")(&e))?;
        write_leaky_note(&docs)?;
        std::os::unix::fs::symlink("/etc/hostname", docs.join("host-link.md"))
            .map_err(|e| setup("corpus")(&e))?;
        let memory_config = MemoryConfig::default();
        let guard = GuardConfig::default();
        let mut memory = Memory::new(
            Arc::new(HashEmbedder::new(512)),
            memory_config,
            guard.clone(),
        );
        let report = memory
            .index(&corpus)
            .await
            .map_err(|e| setup("indexing")(&e))?;
        tracing::info!(
            files = report.files_indexed,
            chunks = report.chunks_indexed,
            quarantined = report.quarantined.len(),
            "corpus indexed"
        );

        // Broker: commands bound to real binaries, and a ledger that survives restarts.
        let (manifests, scratch_path) = demo_manifests(&bin)?;
        let original = std::fs::read(&scratch_path).map_err(|e| setup("scratch binary")(&e))?;
        let key = config
            .ledger_key
            .as_deref()
            .map(LedgerKey::new)
            .transpose()
            .map_err(|e| setup("LEASH_LEDGER_KEY")(&e))?;
        if key.is_none() {
            tracing::warn!(
                "no LEASH_LEDGER_KEY: the ledger is an unkeyed hash chain, so someone who can write the file can rewrite it consistently"
            );
        }
        let ledger = Ledger::open(&data.join("ledger.jsonl"), key)
            .map_err(|e| StartError::Setup { what: "ledger", reason: format!("{e} (delete the data directory to start over, or restore the key it was written with)") })?;
        let broker = Broker::new(manifests, ledger, work_dir.clone(), BrokerConfig::default())
            .map_err(|e| setup("broker")(&e))?;

        let bridge = Bridge::new(Duration::from_secs(12)).map_err(|e| setup("bridge")(&e))?;

        Ok(Self {
            started: Instant::now(),
            gateway,
            local,
            remote,
            memory: RwLock::new(memory),
            memory_config,
            guard,
            corpus,
            broker,
            ledger_key: config.ledger_key.clone(),
            scratch: Scratch {
                path: scratch_path,
                original,
            },
            work_dir,
            bridge,
            config,
        })
    }
}
