# Vidium

A tool to record video from a headless Chrome/Chromium tab.

Uses the [Page.startScreencast](https://chromedevtools.github.io/devtools-protocol/tot/Page/#method-startScreencast) Chrome DevTools protocol method.

## Installation

```
cargo install vidium
```

## Usage 

```
vidium encode --url https://google.com --output google.mp4 --width 800 --height 600 --headless=false
```