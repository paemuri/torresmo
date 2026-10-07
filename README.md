# Torresmo

[![License][badge-1-img]][badge-1-link]
[![crates.io][badge-2-img]][badge-2-link]
[![AUR][badge-3-img]][badge-3-link]

## About

Dead simple and minimal TUI client for Transmission daemon/service.

## Why?

Torresmo is a deliberately tiny alternative to full-featured clients like
[Rustmission][2] or other TUI projects: no configuration, no sorting, no tabs.
It shows your torrents in queue order and lets you manage them, and nothing
else.

## Installation

AUR / Arch Linux:

```sh
$AUR_HELPER -S torresmo
```

Homebrew / macOS:

```sh
brew install paemuri/pkgs/torresmo
```

Nix:

```sh
nix profile install sourcehut:~paemuri/nix-pkgs#torresmo
```

Cargo:

```sh
cargo install torresmo
```

## Usage

Start the Transmission daemon (it must listen on `localhost:9091`, without
authentication), then run:

```sh
torresmo
```

Press `h` inside Torresmo for the key bindings or `q` to exit.

## Ignored features

This project started as a way to implement the features I wanted but couldn't
find in other projects. At the same time, I don't want Torresmo to become a
Swiss Army knife.

If any of the features below are interesting to you, let me know. In the
meantime, nothing will be added unless I see a valid use case for it.

### Table sorting

The table is always ordered by queue position, the `#` column. You can also move
torrents in the queue, which immediately shows in the table's order. Adding
sorting on top of that would add unnecessary complexity, and I don't want that.

### Remote Transmission

To run Torresmo, it's required to have the Transmission daemon running at
`localhost:9091`, without authentication.

### Search and filtering

The number of torrents I have is small enough for me to see all items and not
get lost. If you have so many torrents that you need to search them, I suppose
you'd prefer another project with more features to manage them, anyway.

### Managing files inside a torrent

Too complex for this project's scope.

### Limits and priorities

No speed limits, no bandwidth priority, no "turtle mode". The only existing
concept is the queue order.

## License

This project code is in the public domain. See the [LICENSE file][1].

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you shall be in the public domain, without any
additional terms or conditions.

[1]: ./LICENSE
[2]: https://github.com/intuis/rustmission

[badge-1-img]: https://img.shields.io/badge/license-Unlicense-green?style=flat-square
[badge-1-link]: ./LICENSE
[badge-2-img]: https://img.shields.io/crates/v/torresmo?style=flat-square
[badge-2-link]: https://crates.io/crates/torresmo
[badge-3-img]: https://img.shields.io/aur/version/torresmo?style=flat-square
[badge-3-link]: https://aur.archlinux.org/packages/torresmo
