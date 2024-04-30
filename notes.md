### async downloader


concurrent download for http requires:
- supporting bytes[^1]
    ```rust
    let server_supports_bytes = 
    match headers.get(header::ACCEPT_RANGES) {
        Some(val) => val == "bytes",
        None => false,
    };
    ```
    if ACCEPT RANGES key in header is set to "bytes"
    
- header contains CONTENT LENGTH key[^2]

if server supports bytes and header contains key RANGE (and concurrent) -> remove header RANGE
`why?`

If an HTTP response includes the Accept-Ranges header and its value is anything other than "none", then the server supports range requests.
If they don't support ranges -> Can't stop/resume or chunk download
what about:
```
Range: bytes=900-
```
all bytes after [900, ...]
while limiting the accepted bytes client side? (terminate after n bytes or smth)
so concurrency with
[0-](1024) [1024-](1024) [2048-](1024) [3096-](1024) ...
(1024) bytes are the soft size limit at which the client interrupts the get request somehow, and only use that portion?

request:
```
GET /z4d4kWk.jpg HTTP/1.1
Host: i.imgur.com
Range: bytes=0-1023
```
results in:
```
HTTP/1.1 206 Partial Content
Content-Range: bytes 0-1023/146515
Content-Length: 1024
…
```
so the range is inclusive in both sides: [from, to]  len:(to-from+1)


file io:
- download chunks as seperate files rewrite as whole later `ugly`
- download keep in memory write as a whole `issue w big files`
- mmap?
    - can't share mmap(?) &mut [u8] between tasks? could do actor pattern mpsc but whats the point of mmap then. technically i should be able to share them as theyre distinct regions of memory, but will rust let me - do i have to use unsafe ptrs?. but the regions are created from slices of the whole mmap?
- iouring?
- streams / tokio::io::copy
- fs [write_at](https://doc.rust-lang.org/std/os/unix/fs/trait.FileExt.html#tymethod.write_at)
- streaming the response body chunks it automatically? [1](https://users.rust-lang.org/t/async-download-very-large-files/79621), [2](https://webscraping.ai/faq/reqwest/what-is-the-best-way-to-handle-large-file-downloads-with-reqwest)

reading+sources:

- https://developer.mozilla.org/en-US/docs/Web/HTTP/Range_requests
- https://en.wikipedia.org/wiki/HTTP
    `[ctrl+f "transfer"]`
- https://en.wikipedia.org/wiki/Chunked_transfer_encoding
- https://en.wikipedia.org/wiki/Byte_serving
- https://en.wikipedia.org/wiki/Comparison_of_file_transfer_protocols
- https://crates.io/crates/seekable-async-file
- https://blog.cloudflare.com/scalable-machine-learning-at-cloudflare
- https://www.reddit.com/r/rust/comments/jw7a3s/help_with_lifetimes_using_mmap_chunk_mut/
- https://stackoverflow.com/questions/28516996/how-to-create-and-write-to-memory-mapped-files?rq=3
- https://github.com/seanmonstar/reqwest/issues/482
- https://github.com/benkay86/async-applied/tree/master/reqwest-tokio-compat
- https://users.rust-lang.org/t/fmmap-a-flexible-and-convenient-high-level-wrapper-mmap-for-zero-copy-file-i-o/69785/5
- https://docs.rs/tokio-uring/latest/tokio_uring/fs/struct.File.html
- https://users.rust-lang.org/t/async-file-io-using-memmap/67606


[^1]: reading duma's [source](https://github.com/mattgathu/duma/blob/6e06e9ac83ecb45db1da0fefcc2567ece56af804/src/core.rs#L180C1-L184C1)
[^2]: [source](https://github.com/mattgathu/duma/blob/6e06e9ac83ecb45db1da0fefcc2567ece56af804/src/core.rs#L204)