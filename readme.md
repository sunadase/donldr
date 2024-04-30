

```
cargo run --bin mpsc -- -u https://proof.ovh.net/files/100Mb.dat -c 16
cargo run --release --bin mpsc -- -u https://proof.ovh.net/files/100Mb.dat -c 16
```


```
cargo run --bin mpsc -- -u https://github.com/wez/wezterm/releases/download/20240203-110809-5046fc22/wezterm-20240203-110809-5046fc22-src.tar.gz

wget https://github.com/wez/wezterm/releases/download/20240203-110809-5046fc22/wezterm-20240203-110809-5046fc22-src.tar.gz.sha256

sha256sum ./wezterm-20240203-110809-5046fc22-src.tar.gz


```
missing:

fix noway and compare [1](./notes.md#L56)
-> noway 77s, mspc with 8 chunks 47s