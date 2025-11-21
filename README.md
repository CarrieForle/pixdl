# pixdl

An image downloader. Supporting:
- Pixiv
- Twitter

To enable support for Twitter resources, Microsoft Edge must be installed and [msedgerdriver](https://developer.microsoft.com/en-us/microsoft-edge/tools/webdriver/) must reside in the program directory.

```
pixdl -h
```

```
pixdl is a pixiv illustration downloader.

Usage: pixdl.exe [OPTIONS] [RESOURCES]

Arguments:
  [RESOURCES]  The resources to download

Options:
      --force-login  Start login process on startup
  -h, --help         Print help
  -V, --version      Print version

On startup, pixdl will find "write.txt" in the program directory. pixdl will create it if it couldn't find "write.txt". This is where you put resources to download. You may also supply resources as command line argument.    

A resource is a URL linked to the things you want to download and optionally a bunch of options specific to that resource.

In "write.txt", each resource is separated by a newline. When supplying argument it's separated by comma. The URL and options of a resource are separated by a whitespace.

RESOURCE OPTIONS:
For a Pixiv or Twitter resource, if there are multiple artworks (or subresources) for a given URL, you can optionally specify any number of either range (<start>..<end>) or index of subresources to only download some of the files. The index starts from 1 and the range are inclusive. Not specifying any will download all artworks.      
For example: "https:///www.pixiv.net/artworks/1234 1..2" will download the first and second illustration.
```

# Download

[Windows](https://github.com/CarrieForle/pixdl/releases/latest/download/pixdl.exe)

# Build

Install cargo and Rust. Then do:

```bash
cargo build -r
```