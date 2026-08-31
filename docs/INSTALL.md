# Installation guide

macOS, Windows, Linux

Download the installer for your platform from `https://sovatela.eu`.
That page carries the current release and the SHA-256 for each file; the
installers themselves, and every earlier version, are published as releases on
GitHub, which the page links to.

Before you start: verify the checksum. The download page lists a SHA-256 for
each file — see [Verifying your download](#verifying-your-download).

## How much each platform has been tested

Worth knowing before you download, because it is not the same for all three.

| Platform | Signed | Installed, upgraded and uninstalled on a clean machine |
| --- | --- | --- |
| macOS | Yes — Developer ID, notarized, ticket stapled | Yes |
| Windows | **No** — SmartScreen will warn | **Not yet** |
| Linux (`.deb`, `.rpm`, AppImage) | **No** | **Not yet** |

The application is built and tested on every platform by the automated suite,
and the Windows and Linux packages are inspected after they are built — their
contents, their metadata, and their checksums. What has not happened is someone
sitting at a clean Windows or Linux machine, installing the package, using the
app, upgrading it, and removing it again. Until that has been done, treat those
two as builds you are trying rather than as releases that have been through the
same checks as the macOS one. If something goes wrong there, please
[say so](SUPPORT.md) — you will be the first to find it.

---

## macOS

**Requires macOS 10.15 (Catalina) or later.** One universal `.dmg` covers both
Apple Silicon and Intel Macs.

1. Download the `.dmg` — there is one, `Sovatela_<version>_universal.dmg`.
2. Open the `.dmg` and drag **Sovatela** into **Applications**.
3. Eject the disk image and launch Sovatela from Applications.

The build is signed and notarized with an Apple Developer ID.

The first time you open it, macOS asks once: *"Sovatela" is an app downloaded
from the internet. Are you sure you want to open it?* — and, on the same panel,
**"Apple checked it for malicious software and none was detected."** That is
the notarized result, and it is what you should see. Click **Open**. macOS asks
this once per app for anything downloaded from the web, however it is signed.

What you should **not** see is *"cannot be opened because the developer cannot
be verified"*, or a panel with no **Open** button. Either means the build is not
notarized and did not come from us — **stop**, re-download from
`https://sovatela.eu`, and check the checksum.

You can confirm the signature yourself. This is a command, so it runs in
**Terminal** — the app in *Applications → Utilities*, or press <kbd>⌘</kbd> +
<kbd>Space</kbd> and type "Terminal". Paste the line in and press
<kbd>Return</kbd>:

```sh
spctl -a -t exec -vv /Applications/Sovatela.app
# expect: accepted / source=Notarized Developer ID
```

Lines beginning with `#` are notes, not part of the command — you can paste them
or leave them out.

> The `.dmg` itself reports as unnotarized — the notarization ticket is stapled
> to the `.app` inside, which is what macOS checks at launch.

**Keychain prompt.** The first time Sovatela reads your API key, macOS asks for
permission. Click **Always Allow** so it doesn't ask every launch.

---

## Windows

**Requires Windows 10 (1803) or later, 64-bit.**

Download the **`.exe`** (`Sovatela_<version>_x64-setup.exe`) and run it. That's the one you want.

An `.msi` is also published (`Sovatela_<version>_x64_en-US.msi`) for managed
deployment via Group Policy. Both install the same application; if you don't
know which you need, take the `.exe`.

1. Download the installer.
2. Run it and follow the prompts.
3. Launch Sovatela from the Start menu.

**SmartScreen.** Windows code signing is not yet configured for this project, so
the installer carries no publisher name. Until it does, SmartScreen will show
*"Windows protected your PC"* and an *Unknown publisher*. Click **More info → Run anyway** only if the checksum
matches. This warning is expected and will disappear once the build is signed;
see [`SECURITY.md`](../SECURITY.md).

**WebView2.** Sovatela renders its interface with Microsoft Edge WebView2, which
ships with Windows 10/11. On older or stripped-down installs the app may fail to
start with a blank window — install the [WebView2 Evergreen
Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) and try
again.

---

## Linux

**Requires a distribution with WebKitGTK 4.1** — Ubuntu 22.04 or later, Debian
12+, Fedora 36+, and equivalents.

### Debian / Ubuntu (`.deb`)

```sh
sudo apt install ./sovatela_*_amd64.deb
```

### Fedora / RHEL (`.rpm`)

```sh
sudo dnf install ./Sovatela-*.x86_64.rpm
```

### Any distribution (`.AppImage`)

```sh
chmod +x Sovatela_*_amd64.AppImage
./Sovatela_*_amd64.AppImage
```

### Required system packages

The keychain integration uses the freedesktop Secret Service, which needs a
running keyring daemon (GNOME Keyring or KWallet). On a minimal install:

```sh
# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-0 gnome-keyring libsecret-tools

# Fedora
sudo dnf install webkit2gtk4.1 gnome-keyring libsecret

# Arch
sudo pacman -S webkit2gtk-4.1 gnome-keyring libsecret
```

Without a keyring daemon the app starts but cannot store your API key.

**Wayland note.** If the window renders blank under Wayland, launch with
`WEBKIT_DISABLE_COMPOSITING_MODE=1`. This is a WebKitGTK issue, not a Sovatela
one.

**No repository yet.** There is no apt/dnf repo or Flatpak, so `.deb` and
AppImage installs do not auto-update. Check `https://sovatela.eu` for
new versions.

---

## Verifying your download

Each release lists a SHA-256 for every file, both on the download page and as a
`SHA256SUMS.txt` attached to the release itself. Compare it against your
download.

These are commands. On **macOS** run them in **Terminal** (*Applications →
Utilities*, or <kbd>⌘</kbd> + <kbd>Space</kbd> → "Terminal"); on **Windows** in
**PowerShell** (right-click Start → *Terminal* or *Windows PowerShell*); on
**Linux** in your terminal emulator. `cd` to wherever the file downloaded —
usually `cd ~/Downloads` — before running it.

```sh
# macOS
shasum -a 256 Sovatela_*_universal.dmg

# Linux
sha256sum Sovatela_*_amd64.deb

# Windows (PowerShell)
Get-FileHash .\Sovatela_*_x64-setup.exe -Algorithm SHA256
```

If the value doesn't match, delete the file and download it again from
`https://sovatela.eu`. Don't run it.

---

## After installing

Sovatela needs a **Scaleway API key** before it can do anything — the first
screen walks you through it. See the
[Quick-start guide](QUICKSTART.md).

## Updating

Sovatela has **no automatic updater** in this version. To update, download the
new installer and install over the top; your settings, history, memory, and
projects are preserved. On macOS, replace the app in Applications.

## Uninstalling

See [Uninstall and data deletion](UNINSTALL.md).
