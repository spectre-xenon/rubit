<div align="center">

  <h3 align="center">Rubit</h3>

  <p align="center">
    A simple Bittorrent client written in rust!

  </p>
</div>

## About The Project

So, I was feeling kind of geeky one day and decided I wanted to really get how the internet works under the hood. I'm always down to learn something new, and messing around with a low-levelish language sounded like fun. That's when it hit me—why not try building a BitTorrent client with Rust?

It's not perfect (yet!), but let me tell you, this whole thing has been a wild ride through the awesome (and sometimes confusing) world of bytes and streams!

## Installation

If you have rust installed you can get it easily with:

```sh
 cargo install rubit-cli
```

_alternatively you can get the executable for your system from the releases tab._

## Usage

To start a download use the following command

```sh
rubit -t <path to .torrent file>
```

To specify an output location and name you can for example use

```sh
rubit -t <path to .torrent file> -o ~/Download/test.mkv
```

And finally if you find the download speed too slow you can us the `-i` flag to change the interval (in Seconds) at which the client requests new peers from the tracker

## Roadmap / Features

- [x] Decode Bencode
- [x] torrent-file struct
  - [x] single-file
  - [ ] multi-file
- [x] Tracker Struct
  - [x] http tracker announce
  - [x] udp tracker announce
- [x] Peer wire protcol
  - [x] Message struct with implementation to generate correct buffers for each message
  - [x] implent main loop for tcp communication with peers
- [x] Multi-threading
  - [x] handle each peer in a thread
  - [x] handle a global queue of peices and HashSet of peers
- [x] File-system
  - [x] handle writing different pieces at different offsets correclty
  - [ ] Multi-file writing
- [x] Main cli binar
  - [x] beautify with a simple nice progress bar

## Contributing

If you have a suggestion that would make this better, please fork the repo and create a pull request. You can also simply open an issue.
Don't forget to give the project a star! Thanks again!

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## License

Distributed under the MIT License. See `LICENSE.txt` for more information.
