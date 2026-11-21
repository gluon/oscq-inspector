# Download and first launch

Formerly void-oscq, shown as Void OSCQ: the versions published under that name, 0.2.3 and earlier, keep it in their downloads, `void-oscq-VERSION-...`, which hold **Void OSCQ.app** on macOS. It is the same application under a new name, and the downloads named below are those of the versions after 0.2.3.

Each version is on the [Releases page](https://github.com/gluon/oscq-inspector/releases), with one file per system. The apps are not signed: your system warns you the first time you open one, and the steps below get past that warning once.

Every download carries the license texts: the GNU GPL, its additional permission and the licenses of the bundled fonts, in a `licenses` folder, inside `OSCQuery Inspector.app/Contents/Resources` on macOS.

## macOS 12 or later, Apple Silicon or Intel

Download `oscq-inspector-VERSION-macos-universal.zip`. Unzip it to get **OSCQuery Inspector.app**, and move it to your Applications folder.

The app is not signed by an identified developer, so macOS stops it the first time:

1. Double click **OSCQuery Inspector**. macOS says it cannot verify the app. Close that dialog: **Done**, **OK** or **Cancel** depending on the macOS version, never **Move to Trash**.
2. Open **System Settings**, then **Privacy & Security**. Scroll down to **Security**: a line says OSCQuery Inspector was blocked. Click **Open Anyway**, then confirm with your password or Touch ID.
3. Click **Open** in the last dialog. From now on the app opens normally.

On macOS 12 to 14 there is a shorter way: Control click the app, choose **Open**, then **Open** again.

The first time the app looks for servers, macOS asks whether OSCQuery Inspector may find devices on your local network. Click **Allow**. If you clicked **Don't Allow**, or if no dialog appeared, turn it on in **System Settings**, **Privacy & Security**, **Local Network**. A server on the same Mac at `localhost` works either way; one on the same Mac advertised on its Wi-Fi or Ethernet address does not, which is why a window can show one server of your own machine and none of the others.

Until something answers, the **Discovered** panel says what it has been able to do and no more: that it is waiting for permission, with where to give it, while the system is refusing its queries; that it is searching and nothing has answered, with where to look, while they go out unanswered. It says that none was found only once a server has really answered. Once you allow it, servers appear within a few seconds, and you do not have to quit and open the app again: it keeps a query going out every few seconds until one is answered.

**Updating.** macOS decides this for each build of an application, not for its name: a new version is a new application to it, so it asks again, and **Local Network** lists the version you had and the one you installed. Allow the new one. Turning the old entry off changes nothing for the new one, and you can leave both on or remove the old entry; the settings, the manual entries and the preset slots are kept either way. The dialog does not always appear on an update: if the panel says nothing has answered while a server is running, that switch is the first place to look.

## Windows 10 or later, 64 bit

Download `oscq-inspector-VERSION-windows-x86_64.zip` and unzip it: a folder holding `oscq-inspector-VERSION-windows-x86_64.exe` and a `licenses` folder. The `.exe` is the whole application: no installer, put it where you like.

The program is not signed, so Windows SmartScreen stops it the first time:

1. Double click the `.exe`. A blue window says **Windows protected your PC**.
2. Click **More info**, then **Run anyway**.

If Windows Defender Firewall asks whether to allow OSCQuery Inspector on networks, allow it on **private networks**: discovery and the values pushed by servers need it.

## Linux, x86_64

Download `oscq-inspector-VERSION-linux-x86_64.tar.gz`, unpack it and run the binary:

```
tar -xzf oscq-inspector-VERSION-linux-x86_64.tar.gz
cd oscq-inspector-VERSION-linux-x86_64
./oscq-inspector
```

To have it in your application menu with its icon, copy the binary to `~/.local/bin`, the `.desktop` file to `~/.local/share/applications`, and `oscq-inspector.png` to `~/.local/share/icons/hicolor/256x256/apps`.

What the system needs to run it: X11 or Wayland, `libxkbcommon-x11-0` on X11, and a Vulkan or OpenGL driver (Mesa is enough). It needs glibc 2.35 or newer, as on Ubuntu 22.04 or later. The release build prints this requirement, measured on the binary.
