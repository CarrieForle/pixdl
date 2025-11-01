# pixdl

A [Pixiv](https://www.pixiv.net/) illustration downloader. Account is not required.

```
pixdl -h
```

```
Usage: pixdl.exe

DESCRIPTION
pixdl is a pixiv illustration downloader.

USAGE
On startup, pixdl will find "write.txt" in the program directory. pixdl will create it if it couldn't find "write.txt". This is where you put resources to download.

A resource is a URL linked to the things you want to download and optionally a bunch of options specific to that resource. There is only one kind of resource: pixiv. In the future I might support more.

In "write.txt", each resource is separated by a newline. Each line contains a URL and optionally some options. The URL and each option are separated by a whitespace.

In a pixiv resource, the URL should looks like "https://www.pixiv.net/artworks/<illust_id>". If there are multiple artworks for a given URL, you can optionally specify either a range (<start>..<end>) or any number of index of illustration to only download some of the files. The index starts from 1 and the range are inclusive. Not specifying any will download all artworks.

For example: "https://www.pixiv.net/artworks/1234 1..2" will download the first and second illustration.
```

# Download

[Windows](https://github.com/CarrieForle/pixdl/releases/latest/download/pixdl.exe)

# Build

Install cargo and Rust. Then do:

```bash
cargo build -r
```