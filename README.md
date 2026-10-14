# base-camp-linux

Linux control for Mountain peripherals, starting with the **Everest Max** keyboard. It covers
preset and per-key RGB, plus macros stored on the keyboard. It talks to the device through
the kernel's `hidraw`; you don't need a driver, Wine or the Windows service.

The protocol was traced from Mountain Base Camp 1.9.10. See:

* [docs/reverse-engineering-report.md](docs/reverse-engineering-report.md): architecture,
  how macros and RGB reach the device, and what is proven, inferred and still unknown
* [docs/protocol-everest-max.md](docs/protocol-everest-max.md): byte-level packet reference

> **Status:** packet layouts are reproduced from the shipped SDK and covered by golden-byte
> tests. They have **not** been tried on a physical keyboard yet. Start without `--save`,
> which writes to the keyboard's flash.

## Install

```sh
pip install --user .
sudo cp packaging/70-mountain-basecamp.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

## Use

```sh
basecamp-linux list                                  # Mountain hidraw nodes + vendor report id
basecamp-linux --dry-run effect static -c ff0000     # print packets only
basecamp-linux effect breathing -c 00a0ff -s 60
basecamp-linux perkey --default 101010 --set Esc=ff0000 --set W=00a0ff
basecamp-linux macro F12 examples/macros/hello.json  # stored on the keyboard
basecamp-linux apply examples/profile.json
basecamp-linux keys                                  # key names, firmware codes, LED indices
```

To re-apply a profile every time the keyboard is plugged in:

```sh
mkdir -p ~/.config/basecamp-linux && cp examples/profile.json ~/.config/basecamp-linux/
cp packaging/basecamp-linux.service ~/.config/systemd/user/
systemctl --user enable --now basecamp-linux
```

## Test

```sh
python3 -m unittest discover -s tests
```

## Before you report a bug

Attach the output of `basecamp-linux list` and of
`sudo xxd /sys/class/hidraw/hidrawN/device/report_descriptor` for your keyboard's node.
The real report descriptor is the main piece the Base Camp code cannot show.
