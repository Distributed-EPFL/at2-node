## dependencies

AT2 uses a number of algorithms and libraries to implement asset transfer.
You should read the next list as a stack, each library depending on the
previous one.

- [drop](https://github.com/Distributed-EPFL/drop) for low-level network
  plumbing
- [murmur](https://github.com/Distributed-EPFL/murmur) for broadcasting
  messages
- [sieve](https://github.com/Distributed-EPFL/sieve) for consistently
  broadcasting messages, such as filtering double-spending ones
- [contagion](https://github.com/Distributed-EPFL/contagion) for securely
  broadcasting messages, such that every correct node sees the same sequence
  of messages for a given sender

## RPC

The node exposes a gRPC service as described in the `src/at2.proto` file.
