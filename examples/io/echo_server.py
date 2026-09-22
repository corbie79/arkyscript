#!/usr/bin/env python3
"""Echo one TCP client on localhost, then exit when the client disconnects."""
import argparse
import socket


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    with socket.create_server(("127.0.0.1", args.port)) as listener:
        print(f"Echo server: 127.0.0.1:{listener.getsockname()[1]}", flush=True)
        connection, _ = listener.accept()
        with connection:
            while data := connection.recv(4096):
                connection.sendall(data)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
