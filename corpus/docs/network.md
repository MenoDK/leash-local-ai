# Network

## Addresses

The router hands out addresses from the range 192.168.10.100 to 192.168.10.199. The file server and the printer have
fixed addresses below 192.168.10.50 so they never move. The DNS resolver runs on the file server.

## When the internet is down

Check the router first: if its status light is red, power-cycle it and wait two minutes. If the light is green but no
page loads, the resolver is the likely culprit; restart the resolver service on the file server before calling the
provider.

## Guest wireless

Guests use the separate guest wireless network. It reaches the internet and nothing else: the file server and the
printer are not visible from it. The guest password is changed on the first day of each month.
