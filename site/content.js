/* Content for the landing page and the lab. Everything the page says about behaviour is backed by a test; the output in
   the tour is recorded from the running server (scripts/record-transcripts.mjs) and CI fails if it drifts. */
window.SITE = {
  name: 'leash-local-ai',
  short: 'Leash',
  title: 'leash-local-ai · a local AI assistant on a short leash',
  logo: 'plug',
  repo: 'https://github.com/MenoDK/leash-local-ai',
  accent: {
    light: { fill: '#e8624a', ink: '#a3361f', on: '#2b0d06', soft: '#fde4dd', onNavy: '#ff9a85' },
    dark: { fill: '#f0765f', ink: '#ffb3a4', on: '#2b0d06', soft: '#4a1d14', onNavy: '#ff9a85' },
  },
  loading: {
    context: 'Local AI',
    title: 'Clipping on the leash',
    text: 'Checking the seal, the gateway, the broker and the ledger.',
    icon: 'plug',
    satellites: ['lock', 'key', 'hash'],
    foot: 'Local demo · one Rust binary, nothing leaves this machine',
    sketch: 'M10 118C50 118 64 40 108 40S150 128 196 128 244 56 292 56 316 110 332 110',
  },
  lab: { href: 'lab.html', cta: 'Enter the lab' },

  hero: {
    eyebrow: 'Local AI · sealed input, approved action, signed history',
    lead: 'An AI assistant',
    soft: 'on a short leash.',
    text: 'A Rust toolkit for the part of a local assistant that has to be right. Everything the model reads is sealed first, private text cannot reach a remote model even when the local one is down, a command runs only after a person approved exactly what was shown, and the record of what happened notices when it is edited.',
    primary: { label: 'Open the lab', href: 'lab.html' },
    secondary: { label: 'See how it works', href: '#tour', icon: 'layers' },
    pilot: 'Runs on your laptop · one binary, no GPU, no API keys, no accounts',
    proof: [
      { icon: 'lock', a: 'Input is sealed', b: 'by a type, not by a habit' },
      { icon: 'flask', a: 'Every defence is attacked', b: 'in a lab you can click' },
    ],
    top: ['LEASH', 'Reads sealed. Acts approved. Remembers signed.'],
    status: { icon: 'plug', small: 'Last verdict', strong: 'The tool output gave orders. It was held. Nobody saw it.' },
    art: `<svg viewBox="0 50 500 620" role="img" aria-label="A document marked untrusted contains the line ignore previous instructions. It passes through a box labelled seal and reaches a small robot as plain data. A leash runs from the robot's collar down to a hand labelled you, and beside it a card reads: proposal, say hello, needs a person. Below, a chain of four numbered blocks shows the third one edited.">
      <g class="a-line"><path d="M0 120H500M0 240H500M0 360H500M0 480H500M0 600H500M100 30V700M250 30V700M400 30V700"/></g>
      <g transform="translate(0 42)">
      <circle cx="420" cy="132" r="62" class="a-soft"/>

      <g>
        <rect x="26" y="96" width="150" height="112" rx="14" class="a-body"/>
        <rect x="26" y="96" width="150" height="112" rx="14" class="a-line"/>
        <text x="42" y="120" class="a-text-s">TOOL OUTPUT</text>
        <path d="M42 134h104M42 148h82" class="a-line"/>
        <rect x="38" y="160" width="126" height="34" rx="8" class="a-bad-f" fill-opacity="0.3"/>
        <text x="48" y="174" class="a-text-s">ignore previous</text>
        <text x="48" y="188" class="a-text-s">instructions…</text>
      </g>

      <path d="M178 152H236" class="a-dash"/>
      <path d="M228 144l10 8-10 8" class="a-accent-s"/>
      <g>
        <rect x="240" y="116" width="74" height="72" rx="16" class="a-body"/>
        <rect x="240" y="116" width="74" height="72" rx="16" class="a-line"/>
        <path d="M277 130l18 7v12c0 10-7 17-18 21-11-4-18-11-18-21v-12l18-7z" class="a-accent-s"/>
        <text x="277" y="204" text-anchor="middle" class="a-text">SEAL</text>
      </g>
      <path d="M316 152H350" class="a-dash"/>

      <g>
        <rect x="352" y="92" width="92" height="76" rx="18" class="a-body"/>
        <rect x="352" y="92" width="92" height="76" rx="18" class="a-line"/>
        <circle cx="381" cy="126" r="8" class="a-line"/><circle cx="415" cy="126" r="8" class="a-line"/>
        <circle cx="381" cy="126" r="3.2" class="a-accent pulse"/><circle cx="415" cy="126" r="3.2" class="a-accent pulse"/>
        <path d="M376 150h44" class="a-line"/>
        <path d="M398 92V76" class="a-line"/><circle cx="398" cy="70" r="5" class="a-accent"/>
        <path d="M360 174c10 16 74 16 84 0" class="a-accent-s"/>
        <text x="398" y="208" text-anchor="middle" class="a-text-s">reads data, not orders</text>
      </g>
      </g>

      <path d="M402 234C440 268 330 274 270 280S170 302 120 312" class="a-dash"/>
      <g>
        <rect x="66" y="300" width="86" height="64" rx="20" class="a-body"/>
        <rect x="66" y="300" width="86" height="64" rx="20" class="a-line"/>
        <path d="M84 334c0-10 6-14 14-14s14 4 14 14v12h-28z" class="a-line"/>
        <text x="109" y="384" text-anchor="middle" class="a-text">YOU</text>
      </g>

      <g>
        <rect x="236" y="290" width="214" height="100" rx="16" class="a-body"/>
        <rect x="236" y="290" width="214" height="100" rx="16" class="a-line"/>
        <path d="M236 320h214" class="a-line"/>
        <text x="252" y="310" class="a-text-s">PROPOSAL</text>
        <text x="252" y="340" class="a-text-s">say hello</text>
        <text x="252" y="358" class="a-text-s">sha256 of the exact program</text>
        <rect x="252" y="366" width="132" height="14" rx="7" class="a-line"/>
        <circle cx="262" cy="373" r="4" class="a-accent pulse"/>
        <text x="274" y="377" class="a-text-s">needs a person</text>
      </g>

      <g>
        <text x="250" y="428" text-anchor="middle" class="a-text">SIGNED LEDGER</text>
        <path d="M80 482H420" class="a-dash"/>
        <g>
          <rect x="30" y="454" width="82" height="56" rx="10" class="a-body"/><text x="71" y="478" text-anchor="middle" class="a-text-s">#1 proposed</text><text x="71" y="496" text-anchor="middle" class="a-text-s">9f3a…</text>
          <rect x="130" y="454" width="82" height="56" rx="10" class="a-body"/><text x="171" y="478" text-anchor="middle" class="a-text-s">#2 approved</text><text x="171" y="496" text-anchor="middle" class="a-text-s">c71e…</text>
          <rect x="230" y="454" width="82" height="56" rx="10" class="a-body"/><rect x="230" y="454" width="82" height="56" rx="10" class="a-bad-f" fill-opacity="0.3"/><text x="271" y="478" text-anchor="middle" class="a-text-s">#3 edited</text><text x="271" y="496" text-anchor="middle" class="a-text-s">?</text>
          <rect x="330" y="454" width="82" height="56" rx="10" class="a-body"/><text x="371" y="478" text-anchor="middle" class="a-text-s">#4 finished</text><text x="371" y="496" text-anchor="middle" class="a-text-s">e8d0…</text>
        </g>
        <path d="M112 482h18M212 482h18M312 482h18" class="a-accent-s"/>
        <text x="250" y="532" text-anchor="middle" class="a-text-s">each record signs the one before it</text>
      </g>
    </svg>`,
  },

  principles: {
    lead: ['Built to be doubted', 'by the person it serves.'],
    items: [
      ['lock', 'Seals what it reads'],
      ['route', 'Keeps private text home'],
      ['key', 'Asks before it acts'],
      ['hash', 'Signs its history'],
    ],
  },

  tour: {
    eyebrow: 'A tour of the whole thing',
    title: ['Six things that go wrong', 'with a local assistant.'],
    text: 'Pick a tab. The output below is recorded from the running server with fictional data; CI records it again and fails if it ever stops being true.',
    tag: 'Recorded output',
    tabs: [
      {
        id: 'seal',
        label: 'Seal',
        icon: 'lock',
        title: 'What a tool prints is data, never an order.',
        text: 'Escape codes, invisible characters, keys, chat-template tokens and the tag that would close the wrapper are removed before a model sees a byte. The only way to build the type the model accepts is to go through the sealing function.',
        lab: 'lab.html#seal',
        transcript: (window.TRANSCRIPTS || {}).seal || '',
      },
      {
        id: 'route',
        label: 'Route',
        icon: 'route',
        title: 'Private notes do not leave, even when the local model is down.',
        text: 'Every text carries where it may be processed. A remote model is never asked about private text, whatever the state of the others, and a provider that keeps failing is put on hold instead of being hammered.',
        lab: 'lab.html#route',
        transcript: (window.TRANSCRIPTS || {}).route || '',
      },
      {
        id: 'memory',
        label: 'Memory',
        icon: 'book',
        title: 'It answers from your documents, or says it does not know.',
        text: 'Only documents under a docs folder are read. Links, hidden files and anything named like a credential are refused; a passage that talks to the model is held back whole; a secret pasted into a note never comes back out.',
        lab: 'lab.html#memory',
        transcript: (window.TRANSCRIPTS || {}).memory || '',
      },
      {
        id: 'broker',
        label: 'Broker',
        icon: 'key',
        title: 'The model proposes. A person approves exactly what was shown.',
        text: 'There is no shell. A command is a manifest with typed arguments and the hash of one program. Approval is bound to a digest of the exact argument list, works once, and a program that changed on disk does not run.',
        lab: 'lab.html#broker',
        transcript: (window.TRANSCRIPTS || {}).broker || '',
      },
      {
        id: 'ledger',
        label: 'Ledger',
        icon: 'hash',
        title: 'A record that notices when someone edits it.',
        text: 'Every request, refusal, approval and result is a row that signs the row before it, with a key. Editing, reordering and rebuilding are caught by the chain; cutting off the end is caught only by a receipt kept somewhere else, and the lab shows both.',
        lab: 'lab.html#ledger',
        transcript: (window.TRANSCRIPTS || {}).ledger || '',
      },
      {
        id: 'bridge',
        label: 'Bridge',
        icon: 'network',
        title: 'A guest machine that sleeps until the right certificate wakes it.',
        text: 'The host dials; the guest never does. Mutual TLS 1.3, and the agent checks the peer’s role as well as its signature. Until an authenticated activate it answers two questions and nothing else, and it knows four operations, none of which is a command line.',
        lab: 'lab.html#bridge',
        transcript: (window.TRANSCRIPTS || {}).bridge || '',
      },
    ],
    modulesTitle: 'Everything in the box.',
    modulesNote: 'Each one is a crate with its own tests and a place in the lab',
    modules: [
      ['lock', 'leash-censor', 'NFKC normalisation, ANSI and invisible-character stripping, secret redaction, template-token neutralisation, and an instruction screen. The sealed type has private fields and no way to be built by hand; a compile-fail doctest pins that.'],
      ['route', 'leash-gateway', 'An ordered list of providers behind per-provider circuit breakers with one half-open probe. Residency is checked before routing, and the trace of what was skipped, tried and answered comes back with the answer.'],
      ['book', 'leash-memory', 'A path guard that never follows links, a fence-aware Markdown chunker, an exact vector store with a relevance floor, and “I don’t know” as a first-class answer. The embedder here is lexical and says so.'],
      ['key', 'leash-broker', 'Manifests with typed arguments, a pinned binary opened without following links and hashed through the descriptor it will run from, resource limits, a single-use approval, and a ledger written before anything starts.'],
      ['network', 'leash-bridge', 'Length-prefixed typed frames capped before allocation, a state machine that falls dormant without a heartbeat, a demo PKI, and mutual TLS 1.3 with a role check. A hypervisor-socket transport sits behind a feature flag.'],
      ['server', 'leash-server', 'One axum process that wires the five together, serves this site, refuses foreign Host and Origin headers, and exposes the lab only when asked to, never in production.'],
    ],
    transparency: {
      strong: 'Honest about the demo.',
      text: 'The two language models are deterministic stand-ins that do not generate text; point the server at a local Ollama to use a real one. The embedder is lexical. The hypervisor-socket transport compiles but was not run between a real host and guest. There is no sandbox around the child process beyond limits and no_new_privs. The limitations list in the README is complete.',
    },
  },

  challenges: {
    eyebrow: 'The hard parts',
    title: ['Go on,', 'try to get past the leash.'],
    text: 'These are seven things that are easy to claim and hard to get right. Each has a recipe, what you should see, and a button that opens the lab on that exact tab.',
    items: [
      {
        label: 'Give it an order',
        icon: 'lock',
        access: 'Lab · Seal',
        title: 'Hide an instruction in a tool’s output.',
        text: 'Whatever a tool prints was written by whoever the tool talked to.',
        steps: [
          'In <code>Seal</code>, press “An order inside a log line” and then “Seal it”.',
          'Press “Escape codes and a key”, then “Template tokens”.',
          'Finally press “A polite paraphrase” and look at the screen result.',
        ],
        expect: 'The first is <code>held</code> by the rule <code>override-instructions</code>. The key is gone, the colour codes are gone, and the template tokens and angle brackets are neutralised. The paraphrase <em>passes the screen</em>, which is the honest part: the screen is a pattern list, and the real defence is that nothing the model says can run without a person.',
        lab: 'lab.html#seal',
      },
      {
        label: 'Leak the notes',
        icon: 'route',
        access: 'Lab · Route',
        title: 'Kill the local model and ask about private text.',
        text: 'A fallback is a way to break a rule politely.',
        steps: [
          'In <code>Route</code>, set the local model to “Failing”.',
          'Send the public sentence, then the private one. Read both traces.',
          'Send the public sentence twice more and watch the circuit.',
        ],
        expect: 'The public sentence is answered by the remote model. The private one is <code>refused</code>, and the remote model shows <code>skipped_remote</code>: it was never asked. After three failures the local circuit is <code>open</code> and the next call does not even try it.',
        lab: 'lab.html#route',
      },
      {
        label: 'Poison the library',
        icon: 'book',
        access: 'Lab · Memory',
        title: 'Plant an order, a secret and a path trick in the documents.',
        text: 'The documents are the easiest place to hide something.',
        steps: [
          'In <code>Memory</code>, read the index report: what was kept, held and refused.',
          'Ask “How is the staging release deployed?” and read the passage that comes back.',
          'In “Read a file”, try <code>../../etc/passwd</code>, <code>/etc/passwd</code>, <code>docs/host-link.md</code> and <code>docs/passwords-list.md</code>.',
        ],
        expect: 'One passage is held back whole by <code>override-instructions</code>. The deploy note comes back with <code>[REDACTED]</code> where three secrets were. Every path trick is refused with its own code: <code>path_traversal</code>, <code>malformed_path</code>, <code>symlink</code>, <code>not_allowed</code>.',
        lab: 'lab.html#memory',
      },
      {
        label: 'Make it run something',
        icon: 'key',
        access: 'Lab · Broker',
        title: 'Smuggle a flag, a second command, or a stale approval.',
        text: 'The shell is the oldest way to turn a sentence into a catastrophe.',
        steps: [
          'In <code>Broker</code>, propose <code>say</code> with <code>-rf</code>, then <code>a; id</code>, then the command <code>bash</code>.',
          'Propose <code>say hello</code>. Press “Approve with a wrong digest”, then “Approve”, then “Approve” again.',
          'Propose <code>kernel</code> with the flag <code>-s</code>.',
        ],
        expect: 'The first three never become proposals (<code>invalid_arguments</code>, <code>unknown_command</code>). The wrong digest is <code>409 digest_mismatch</code>, the right one runs once, and the second approval is <code>already_resolved</code>. The read-only command runs by policy and its output arrives sealed.',
        lab: 'lab.html#broker',
      },
      {
        label: 'Swap the program',
        icon: 'alert',
        access: 'Lab · Broker',
        title: 'Change the file between “approved” and “run”.',
        text: 'The path you approved is not the program that runs.',
        steps: [
          'In <code>Broker</code>, press “Change the file before the request”.',
          'Press “Change the file after I approve”.',
          'Propose <code>say hello</code> again to confirm the original is back.',
        ],
        expect: 'The first is refused as <code>binary_changed</code> before a proposal exists. In the second a person approved, and the proposal ends <code>failed</code> with “the binary changed after the proposal was made”: the hash is checked again on the file descriptor that would have been executed.',
        lab: 'lab.html#broker',
      },
      {
        label: 'Rewrite history',
        icon: 'hash',
        access: 'Lab · Ledger',
        title: 'Edit, reorder, rebuild or cut off the record.',
        text: 'A log you can edit quietly is decoration.',
        steps: [
          'In <code>Ledger</code>, press “Verify” on the real record.',
          'Forge a copy four ways: edit, reorder, rebuild without the key, cut the end.',
          'Read “caught by” on each card.',
        ],
        expect: 'Edit, reorder and rebuild are caught by <code>the chain</code>. Cutting off the end leaves a perfectly valid, shorter chain and is caught only by <code>the anchor</code>, the length and head hash kept elsewhere. The real record is never touched.',
        lab: 'lab.html#ledger',
      },
      {
        label: 'Pose as the controller',
        icon: 'network',
        access: 'Lab · Bridge',
        title: 'Take over a sleeping agent with a credential that should not work.',
        text: 'Being on the right network is not an identity.',
        steps: [
          'In <code>Bridge</code>, press “Activate” and send a ping.',
          'Press each of the five impostors.',
          'Send a ping again from the real controller.',
        ],
        expect: 'Each impostor is turned away and the agent stays dormant: a wrong role is refused at the application, another authority’s certificate, an expired one, none at all and plain text are refused at the handshake. The real session carries on. There is no operation that takes a command line.',
        lab: 'lab.html#bridge',
      },
    ],
  },

  run: {
    eyebrow: 'Run it in a minute',
    title: ['One command.', 'A leashed assistant on localhost.'],
    text: 'You need Docker. There are no API keys, no model to download and no accounts to create; nothing leaves your machine.',
    steps: [
      ['terminal', 'Clone', 'git clone, then cd into the folder.'],
      ['play', 'Start', 'make up writes a .env with a fresh random ledger key, builds the image and waits until it is healthy.'],
      ['globe', 'Open', 'http://localhost:8090 is this page. Press “Enter the lab”.'],
      ['flask', 'Break it', 'Use the hard parts above, or run make smoke for a twelve-step check against the running server.'],
    ],
    terminal: {
      title: 'bash',
      lines: [
        '# 1. get the code',
        'git clone https://github.com/MenoDK/leash-local-ai.git',
        'cd leash-local-ai',
        '# 2. start the server (builds a small image, creates .env)',
        'make up',
        '# 3. open http://localhost:8090 and press “Enter the lab”',
        '# optional: the same checks from a terminal',
        'make smoke',
      ],
    },
  },

  under: {
    eyebrow: 'Under the hood',
    title: ['A small surface,', 'serious edges.'],
    text: 'The interesting code is short on purpose. The decisions behind it are written down with the alternatives that lost.',
    stackTitle: 'What it is built with',
    stack: [
      { group: 'Language', items: ['Rust 2024 · stable', 'unsafe denied, one audited exception', 'no unwrap, expect or panic in libraries', 'clippy with warnings as errors'] },
      { group: 'Runtime', items: ['tokio', 'axum 0.8', 'serde with deny_unknown_fields', 'no database: a file you can read'] },
      { group: 'Security', items: ['rustls · TLS 1.3 · mutual', 'O_NOFOLLOW · hash through the fd', 'no_new_privs · rlimits', 'HMAC-SHA256 hash chain'] },
      { group: 'Quality', items: ['cargo test · paused-time async tests', 'compile-fail doctest', 'Playwright + axe', 'a sabotage script that must be caught'] },
      { group: 'Delivery', items: ['Multi-stage Dockerfile, non-root', 'Compose, health-gated', 'GitHub Actions · pinned gitleaks'] },
    ],
    proofTitle: 'What is measured, and the records behind it',
    numbers: [
      ['215', 'passing tests'],
      ['17', 'sabotages of a defence, each caught by a test'],
      ['7', 'decision records, each with rejected alternatives'],
      ['0', 'clippy warnings with every lint on'],
    ],
    decisions: [
      ['The model only reads sealed input', 'ADR 0001: why the safe path is the only path the compiler allows.', 'docs/adr/0001-the-model-only-reads-sealed-input.md'],
      ['Private text fails closed', 'ADR 0002: why there is no fallback for confidential text, and what the circuit breaker is for.', 'docs/adr/0002-private-text-fails-closed.md'],
      ['A broker instead of a shell', 'ADR 0003: manifests, a pinned binary and approval bound to a digest.', 'docs/adr/0003-a-broker-instead-of-a-shell.md'],
      ['A signed ledger, and its limits', 'ADR 0004: what the chain proves, what only an anchor can.', 'docs/adr/0004-a-signed-ledger-and-its-limits.md'],
    ],
    measuredOn: 'Measured on 2026-11-04. Counts come from the commands in the README, not from memory.',
    future: [
      ['A real sandbox around the child', 'Landlock and seccomp, so an approved program is also a confined one.'],
      ['Receipts in write-once storage', 'so the anchor lives where the writer cannot reach.'],
      ['A neural embedder behind the same trait', 'for retrieval that understands paraphrase.'],
      ['The hypervisor transport on real hardware', 'between a host and a guest, not just compiled.'],
    ],
  },

  about: {
    why: 'Leash is the local-AI side of my work: putting a model to use on a machine that holds real things, and keeping a person in charge of what it may touch. The ideas come from systems I have built and operated; the code in this repository is written fresh and tested.',
  },

  faq: [
    ['Do I need a GPU, a model or an API key?', 'No. Two deterministic stand-ins answer in place of language models so every defence can be exercised on any laptop. Point LEASH_OLLAMA_URL at a local Ollama to use a real one; the gateway derives “local” from the address itself and never from a setting.'],
    ['Is the model the judge of what is dangerous?', 'No, and that is the point. The instruction screen is a list of patterns, and the lab shows a paraphrase that walks past it. What holds is that nothing a model says can run without a person approving the exact argument list, so a fooled model costs a refused proposal.'],
    ['Why not a shell tool with a deny list?', 'Because a deny list names what you thought of. A manifest names what is allowed: one program by hash, typed arguments, fixed flags, no way to say anything else. ADR 0003 has the alternatives.'],
    ['Does the bridge really work between a VM and a host?', 'The protocol, the TLS and the state machine are tested end to end over an in-process pipe, including the five impostors. The hypervisor-socket transport behind the vsock feature compiles in CI but has not been run between a real host and guest. The README says so.'],
    ['Can the lab mode reach production?', 'No. The server refuses to start with LEASH_LAB set when LEASH_ENV is production, a test pins that, and without the flag every lab endpoint answers 404. The API also refuses any Host or Origin it does not own, because a local tool with no login must not be drivable by a web page.'],
    ['Is it a sandbox?', 'No. Child processes get no_new_privs, resource limits, a cleared environment, their own process group and a hard timeout; they do not get namespaces, seccomp or Landlock. That is listed under “Not built yet”.'],
  ],

  closing: {
    eyebrow: 'Your turn',
    title: ['Try to get', 'past the leash.'],
    button: 'Open the lab',
    fine: 'Local only · fictional data · nothing is sent anywhere',
  },
};
