# ifdyndnsd

Watches Linux netlink for interface addresses and their changes to
perform DynDNS like nsupdate.

## Usage

Pass your configuration file as argument:

```shell
cargo run config.toml
```

## Configuration

```toml
# Define a key which can be referenced by "mykey"
[keys.mykey]
# IP address of the DNS server
# (sorry, can't resolve this yet)
server = "192.0.2.53"
# Key name which needs to match the name configured for the key in the
# DNS server
name = "mykey.example.com"
# Algorithm to use for TSIG
alg = "hmac-sha256"
# Plain-text secret; exclusive with `secret-base64`
secret = "topsecret\n"
# Secret encoded in base64 just like BIND uses. This allows using
# binary strings.
secret-base64 = "dG9wc2VjcmV0Cg=="

# Update an A record (IPv4)
[[a]]
# Select `[key.mykey]` for server/key settings
key = "mykey"
# DNS name to update
name = "dyndns.example.net"
# Network interface to watch for an IP address
interface = "ppp0"
# # Optionally select the proper IP address by subnet. This is the
# # default for IPv4:
# scope = "0.0.0.0/0"

# Update a AAAA record (IPv6)
[[aaaa]]
# Select `[key.mykey]` for server/key settings
key = "mykey"
# DNS name to update
name = "dyndns.example.net"
# Network interface to watch for an IP address
interface = "ppp0"
# # Optionally select the proper IP address by subnet. This is the
# # default for IPv6, which filters for global addresses but not
# # link-local, ULA, ...:
scope = "2000::/3"

# Update another AAAA record
# (when you have setup prefix delegation so that you distribute the
# dynamic ISP-assigned subnet to your LAN)
[[aaaa]]
key = "mykey"
# For IPv6 ifdyndnsd doesn't need to run on the LAN gateway.
name = "router.home.example.net"
# Watch the local LAN interface for addresses
interface = "eth0"
# Default `scope = "2000::/3"` should be right in most DynDNS
# scenarios.

# Once the IPv6 address is known, ifdyndnsd can also update DNS
# records for other hosts in your LAN if you know the last 64 bits of
# their radvd-assigned addresses (the host part in a /64 network). Be
# careful not to use /temporary/ addresses (Privacy Extensions) here:
neighbors."server.example.net" = "::2de:adff:fe00:beef"
neighbors."laptop.example.net" = "::2de:caff:fefb:ad00"
neighbors."phone.example.net" = "::212:23ff:fe56:789a"

```
