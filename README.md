# at2-node

[![at2-node](https://github.com/Distributed-EPFL/at2-node/actions/workflows/rust.yml/badge.svg)](https://github.com/Distributed-EPFL/at2-node/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/Distributed-EPFL/at2-node/branch/master/graph/badge.svg)](https://codecov.io/gh/Distributed-EPFL/at2-node)

An implementation of a distributed ledger using
[AT2](https://arxiv.org/abs/1812.10844) (Asynchronous Trustworthy Transfers).
It you allows to send a certain amount of an asset to a chosen identity
provided that you have enough in your account, while being byzantine resistant.

## overview

There are two binaries, one for the server and one for the client, both in rust.
You can install both with `cargo install --path .`.

### server

```bash
# generate a server config
server config new 127.0.0.1:300{1,2} > server-config

# extract your shareable node information
server config get-node < server-config

# get the others nodes information
cat other nodes informations >> server-config

# start the node
server run < server-config
```

### client

```bash
# generate a client config
client config new http://127.0.0.1:3001 > client-config

# get the recipient public key
recipient=0123456789abcdef

# send some asset
client send-asset $recipient 99 < client-config
```

## roadmap

See the issues for up-to-date advances.

- [ ] confirm transaction
- [ ] handle account per client
- [ ] catchup mechanism for the accounts
- [ ] store state on disk to restart after crash
- [ ] add observability
- [ ] deploy network of node
