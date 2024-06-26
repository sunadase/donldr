mmap:
```
cargo run --bin donldr -- -u https://proof.ovh.net/files/100Mb.dat -c 16
cargo run --release --bin donldr -- -u https://proof.ovh.net/files/100Mb.dat -c 16
```

mpsc:
```
cargo run --bin mpsc -- -u https://proof.ovh.net/files/100Mb.dat -c 16
cargo run --release --bin mpsc -- -u https://proof.ovh.net/files/100Mb.dat -c 16
```

noway:
(This version is not chunked but async streamed with tokio. After seeing it being recommended, I was wondering if it would chunk on its own somehow but doesn't seem like it.)
```
cargo run --bin noway -- -u https://proof.ovh.net/files/100Mb.dat
```

```
cargo run --bin donldr -- -u https://github.com/wez/wezterm/releases/download/20240203-110809-5046fc22/wezterm-20240203-110809-5046fc22-src.tar.gz

cargo run --bin mpsc -- -u https://github.com/wez/wezterm/releases/download/20240203-110809-5046fc22/wezterm-20240203-110809-5046fc22-src.tar.gz

wget https://github.com/wez/wezterm/releases/download/20240203-110809-5046fc22/wezterm-20240203-110809-5046fc22-src.tar.gz.sha256

sha256sum ./wezterm-20240203-110809-5046fc22-src.tar.gz
```

todo:
- comparisons + avgd cmp
- >noway 77s, mspc with 8 chunks 47s

mpsc scheme:
```
www.web.com/data.zip
            ┌─────────┐┌─────────┐┌─────────┐┌─────────┐┌─────────┐
            │  chunk1 ││  chunk2 ││  chunk3 ││  chunk4 ││  chunk5 │
            └───┬─────┘└───┬─────┘└───┬─────┘└───┬─────┘└───┬─────┘
tasks/          │   ʌ      │   ʌ      │   ʌ      │   ʌ      │   ʌ    
spawns/        RESP │     RESP │     RESP │     RESP │     RESP │  
channels        │  GET     │  GET     │  GET     │  GET     │  GET: Range: bytes=c4-c5
rx      txs:    v   │      v   │      v   │      v   │      v   │ 
┌──────┐    ┌───────┴─┐┌───────┴─┐┌───────┴─┐┌───────┴─┐┌───────┴─┐ 
│fileio│    │  down1  ││  down2  ││  down3  ││  down4  ││  down5  │          
├──────┤    ├─────────┘├─────────┘├─────────┘├─────────┘├─────────┘ 
│      │    │ <─────── │───────── │──────────┘          │
│      │  <─┘<──────── │───────── │─────────────────────┘
│      │   <───────────┘          │     (bytes, offset)
│      │    <─────────────────────┘
│      │
│      │ file write head [blocking]
│      ├──────────┐
└──────┘          │write head moves (seek) to chunk offset then 
                  v                              writes all bytes
            ┌─────────┐┌─────────┐┌─────────┐┌─────────┐┌─────────┐
            │  chunk1 ││  chunk2 ││  chunk3 ││  chunk4 ││  chunk5 │
            └─────────┘└─────────┘└─────────┘└─────────┘└─────────┘



ɅV, minuscule: ʌv) ”▲ ^v  ▴   ▵△▲ ⮝⏶ ⋏ ⋀

```
mmap scheme:
```
www.web.com/data.zip
            ┌─────────┐┌─────────┐┌─────────┐┌─────────┐┌─────────┐
            │  chunk1 ││  chunk2 ││  chunk3 ││  chunk4 ││  chunk5 │
            └───┬─────┘└───┬─────┘└───┬─────┘└───┬─────┘└───┬─────┘
                │   ʌ      │   ʌ      │   ʌ      │   ʌ      │   ʌ    
               RESP │     RESP │     RESP │     RESP │     RESP │  
                │  GET     │  GET     │  GET     │  GET     │  GET: Range: bytes=c4-c5
                v   │      v   │      v   │      v   │      v   │ 
tasks:      ┌───────┴─┐┌───────┴─┐┌───────┴─┐┌───────┴─┐┌───────┴─┐ 
            │  down1  ││  down2  ││  down3  ││  down4  ││  down5  │
            │         ││         ││         ││         ││         │
            │async GET││         ││         ││         ││         │
            │  await  ││         ││         ││         ││         │
            │         ││         ││         ││         ││         │
            │         ││         ││         ││         ││         │
            │         ││         ││         ││         ││         │
            │         ││         ││         ││         ││         │
            │ptr::copy││         ││         ││         ││         │
            │to mmap  ││         ││         ││         ││         │ 
            ├─────────┘├─────────┘├─────────┘├─────────┘├─────────┘ 
            │*mut ptr  │          │          │          │
mmap        v          v          v          v          v          
    region: ┌──────────┬──────────┬──────────┬──────────┬─────────┐
            │  chunk1  │  chunk2  │  chunk3  │  chunk4  │  chunk5 │
            ├──────────┴──────────┴──────────┴──────────┴─────────┘
            │mmap flush to disk
            v        
          (disk)
```


io_uring scheme(i think??):
```
www.web.com/data.zip
            ┌─────────┐┌─────────┐┌─────────┐┌─────────┐┌─────────┐
            │  chunk1 ││  chunk2 ││  chunk3 ││  chunk4 ││  chunk5 │
            └───┬─────┘└───┬─────┘└───┬─────┘└───┬─────┘└───┬─────┘
                │   ʌ      │   ʌ      │   ʌ      │   ʌ      │   ʌ    
               RESP │     RESP │     RESP │     RESP │     RESP │  
                │  GET     │  GET     │  GET     │  GET     │  GET: Range: bytes=c4-c5
                v   │      v   │      v   │      v   │      v   │ 
            ┌───────┴─┐┌───────┴─┐┌───────┴─┐┌───────┴─┐┌───────┴─┐ 
            │  down1  ││  down2  ││  down3  ││  down4  ││  down5  │
            │         ││         ││         ││         ││         │
            │async GET││         ││         ││         ││         │
            │  await  ││         ││         ││         ││         │
            │         ││         ││         ││         ││         │
            │(async)  ││         ││         ││         ││         │
            │pollwrite││         ││         ││         ││         │
            │at offset││         ││         ││         ││         │
            │         ││         ││         ││         ││         │
            │         ││         ││         ││         ││         │ 
            ├─────────┘├─────────┘├─────────┘├─────────┘├─────────┘ 
            │          │          │          │          │
            │          │          │          │          │                                                        
            v          v          v          v          v                                                
            ┌─────────┐┌─────────┐┌─────────┐┌─────────┐┌─────────┐
            │  chunk1 ││  chunk2 ││  chunk3 ││  chunk4 ││  chunk5 │
            └─────────┘└─────────┘└─────────┘└─────────┘└─────────┘
io_uring enables async file io 
but how to share the file handle between tasks without locking(?) --> mmap?
or do we need to?
need to be able to write at a specific offset of the file
```