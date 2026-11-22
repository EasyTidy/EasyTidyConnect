# EasyTidy Connect

**English** | [中文](README.zh-CN.md)

Tap an NFC tag, and something happens.

EasyTidy Connect is a desktop app that turns ordinary NFC stickers into buttons. Tap a tag with your phone to send a message, open a web page, run an EasyTidy task, or open a personal anniversary card you designed yourself.

It works on its own, and pairs with [EasyTidy](https://github.com/EasyTidy) when you want a tap to tidy your files.

## Features

**Dashboard.** See at a glance whether the app is ready, how many triggers are active, and what ran recently. A short guide walks you through your first tag.

**Triggers.** One trigger is one tag plus one action. Choose what a tap does:

- show a test message, to check that the tag works
- open a web page
- notify another service
- run an EasyTidy task or workflow
- open an anniversary card

Turn triggers on or off, edit them, and see which tags have been written and confirmed working.

**Tags.** Get everything you need to write a tag with your phone: a link to copy, a QR code to scan, and a step-by-step helper page. The first successful tap marks the tag as confirmed.

**Anniversary Studio.** Make a card that opens when someone taps a tag.

- Write a title, opening, message and closing, or let AI draft them from a few keywords and a tone you pick.
- Choose from 10 designs: minimal, polaroid wall, vintage letter, timeline, starry night, red-and-gold wedding invitation, garden arch, magazine cover, party ticket and film album.
- Describe a style in your own words and let AI design a one-of-a-kind template, then keep refining it.
- Add photos, background music and video. Drag to reorder, pick a cover and crop it to fit.
- Watch a live preview of what the recipient will see on their phone.
- Publish it online so it opens anywhere, or keep it on your home Wi-Fi.
- Write the card's address to a tag, or share it to WeChat, Weibo and other apps with a custom share card.

**History.** The last 50 taps, with time, result and any error.

**Settings.** Light or dark mode, window style, device name, start with your computer, and the connection to EasyTidy. When EasyTidy is connected, you can have Connect start and stop together with it.

The app keeps running in the system tray when you close the window, so tags keep working.

## Getting started

1. Download the installer from [Releases](https://github.com/EasyTidy/EasyTidyConnect/releases) and install it.
2. Get an NFC sticker (see [NFC tags](#nfc-tags)) and an NFC writing app on your phone, such as NFC Tools.
3. In **Triggers**, create a "Test message" trigger.
4. Open **Tags**, scan the QR code with your phone, copy the link and write it to the sticker.
5. Tap the sticker. Your phone opens a page showing the result.

Your phone needs to be on the same Wi-Fi as your computer, unless the trigger opens a card you have published online.

To make an anniversary card, open **Anniversary**, fill in the content, pick a design, add photos, then click **Save and publish** and **Write to NFC**.

## NFC tags

Any NFC tag that your phone can read and write will work. Whether a tag works depends only on your phone.

NTAG213, NTAG215 and NTAG216 stickers work on both iPhone and Android. They are cheap, usually well under US$1 each when bought in packs.

A tag only stores one short link. Before you write a tag, the app shows how many bytes the link takes up. Check that your tag holds at least that much:

| Tag | Capacity |
|---|---|
| NTAG213 | 144 bytes |
| NTAG215 | 504 bytes |
| NTAG216 | 888 bytes |

On Android, make sure NFC is turned on in your phone's settings.

## Development

Requirements: [Node.js](https://nodejs.org/) 22+, [Rust](https://rustup.rs/) (stable), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install          # install dependencies
npm run tauri dev    # run the app in development mode
npm run tauri build  # build an installer into target/release/bundle/
cargo test           # run the Rust tests
```

## License

[MIT](LICENSE)
