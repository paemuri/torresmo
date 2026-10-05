# Torresmo

[![License][badge-1-img]][badge-1-link]

## About

Dead simple and minimal TUI client for Transmission daemon/service.

## Install

```sh
cargo install torresmo
```

## Ignored features

### Table sorting

The table is always ordered by queue position, the `#` column. You can also move
torrents in the queue, which immediately shows in the table's order. Adding
sorting on top of that would add unnecessary complexity, and I don't want that.
If you'd like to have sorting anyway, contact me.

### Remote Transmission

To run Torresmo, it's required to have the Transmission daemon running at
`localhost:9091`, without authentication. The reason is simple: no use case. If
you want it, let me know.

## License

This project code is in the public domain. See the [LICENSE file][1].

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you shall be in the public domain, without any
additional terms or conditions.

[1]: ./LICENSE

[badge-1-img]: https://img.shields.io/badge/license-Unlicense-green?style=flat-square
[badge-1-link]: ./LICENSE
