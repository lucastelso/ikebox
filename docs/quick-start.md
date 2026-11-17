# Quick start: connect to the VPN with a profile you received

**[Português](pt-BR/guia-rapido.md)** · English

This guide is for people who **received a profile file** (`something.json`)
from a colleague and want to connect to the VPN from Linux. It takes about 5
minutes the first time.

You will need:

- the **IPsec Client installer** (`.rpm`, `.deb` or `.AppImage`): from the
  repository's Releases page or from whoever gave you the profile;
- the **profile file** (`something.json`);
- the **group PSK** (the "group key"): ask whoever gave you the profile,
  over a safe channel;
- your VPN **username** and **password**;
- the computer's **administrator password**, only once.

---

## 1. Install IPsec Client (once)

In the folder where the installer is:

- **Fedora:** `sudo dnf install ./ipsec-client-*.x86_64.rpm`
- **Ubuntu/Debian:** `sudo apt install ./ipsec-client_*_amd64.deb`
- **Other distributions:** use the AppImage, as in section 2 of the
  [installation guide](installation.md#other-distributions-appimage).

After that, **IPsec Client** shows up in the applications menu. It speaks the
language of your system (English or Portuguese); you can change it in
**System → Appearance → Language**.

## 2. Install the network component (once)

Open **IPsec Client** from the menu. The main screen shows *"One step left:
install the network component"*. Click **Install now** and type the
**administrator password**. After that, connecting no longer asks for it.

## 3. Import the profile (once)

Go to **Profiles → Import** and choose the file you received, or **drag the
file** into the window. Check the preview and click **Import**.

## 4. Connect (every day)

In **Connection**, click **Connect**. The first time, the app asks for:

- **Group PSK:** the group key;
- **Username:** your VPN username (e.g. employee id);
- **Password:** your VPN password.

Check **Remember in the system keyring** so you do not have to type them
again. The top of the window shows the state by its color: amber while
connecting, **green** with **Connected** when the VPN is on (open Remmina, or
whatever you use, as usual), and **red** when it did not connect: the card
below says what to do.

Under the state, a badge may say **Legacy security** or **Weak security**.
It is a warning about how the VPN is set up (old algorithms or modes), not
an error: the VPN still works. The reasons are in **Profiles**, when you open
the profile; only whoever runs the gateway can change them.

**To disconnect:** the **Disconnect** button (or the tray icon). Closing the
window does not disconnect.

---

## Something went wrong?

| Message | What to do |
|---|---|
| "Wrong XAuth username or password" | Click **Enter credentials** in the alert and type them again |
| "The gateway rejected phase 1 authentication" | The group PSK is wrong: **Enter credentials** and type it again |
| "The gateway did not answer" | Check your internet connection. Some networks (hotels, mobile) block VPNs |
| "UDP ports 500/4500 in use" | Another IPsec VPN is on; turn it off first |
| "This computer is not ready to connect yet" | Click **Install network component** in the alert itself |

Other cases are in the [installation guide](installation.md#common-problems).
The **System → Diagnostics** tab shows what is missing, with a hint on how to
fix it. If you need help, send what shows up there and the connection log
copied with **Copy anonymized**: it has no password or PSK, and the gateway,
IPs and domains become placeholders.

---

## For whoever passes the profile on

In **Profiles**, click the export button (the down arrow) next to the profile
and save the file. It has **no** password, username or PSK.

Send the file, the installer and the link to this guide. The group PSK goes
**over a different channel**, never together with the file nor in an open chat
group.

> Prefer the terminal? There is `ipsec-client-cli`, with the same commands:
> see [docs/cli.md](cli.md).
