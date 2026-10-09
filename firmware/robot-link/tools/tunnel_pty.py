#!/usr/bin/env python3
"""The P4's raw UART tunnel as a local serial port, for the Rust hardware layer.

    python3 firmware/robot-link/tools/tunnel_pty.py 192.168.1.50 [--link /tmp/fpga]

Opens TCP port 4196 on the P4 and a pseudo-terminal whose name is linked at
--link, then copies bytes both ways until either side closes. Point the
calibration server's "serial" at the link (for example a copy of the leg's
server.json). It does the same job as

    socat pty,link=/tmp/fpga,raw,echo=0 tcp:<p4>:4196

for machines without socat.

The P4 grants the tunnel only while no page holds the link, and sends the
FPGA a STOP when the tunnel closes. The FPGA's own leases stop the servos if
this process, the Wi-Fi or the P4 goes away mid-motion.
"""
import argparse
import os
import select
import socket
import sys
import termios
import tty


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("host", help="the P4's address (its USB log prints 'station address ...')")
    ap.add_argument("--port", type=int, default=4196)
    ap.add_argument("--link", default="/tmp/fpga", help="symlink to create for the serial device")
    args = ap.parse_args()

    sock = socket.create_connection((args.host, args.port), timeout=5)
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    sock.settimeout(None)
    # The P4 refuses a second owner with one text line, then closes.
    sock.setblocking(False)
    r, _, _ = select.select([sock], [], [], 0.3)
    if r:
        first = sock.recv(256)
        if not first or first.startswith(b"BUSY"):
            sys.exit(f"tunnel refused: {first.decode(errors='replace').strip() or 'closed'}")
        pending = first
    else:
        pending = b""

    master, slave = os.openpty()
    tty.setraw(slave)
    attrs = termios.tcgetattr(slave)
    attrs[3] &= ~termios.ECHO
    termios.tcsetattr(slave, termios.TCSANOW, attrs)
    name = os.ttyname(slave)
    try:
        os.unlink(args.link)
    except FileNotFoundError:
        pass
    os.symlink(name, args.link)
    print(f"tunnel {args.host}:{args.port} <-> {args.link} ({name}); Ctrl-C to close", flush=True)
    if pending:
        os.write(master, pending)

    try:
        while True:
            r, _, _ = select.select([sock, master], [], [])
            if sock in r:
                data = sock.recv(4096)
                if not data:
                    print("the P4 closed the tunnel", flush=True)
                    break
                os.write(master, data)
            if master in r:
                try:
                    data = os.read(master, 4096)
                except OSError:
                    continue  # no process has the serial side open yet
                sock.sendall(data)
    except KeyboardInterrupt:
        pass
    finally:
        sock.close()
        try:
            os.unlink(args.link)
        except FileNotFoundError:
            pass


if __name__ == "__main__":
    main()
