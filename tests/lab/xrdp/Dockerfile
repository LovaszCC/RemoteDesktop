# xrdp with an XFCE session, a Linux RDP target.
FROM debian:12-slim
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      xrdp xorgxrdp xfce4 xfce4-terminal dbus-x11 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -m -s /bin/bash tester \
 && echo 'tester:tester-pass' | chpasswd \
 && echo xfce4-session > /home/tester/.xsession \
 && chown tester:tester /home/tester/.xsession
EXPOSE 3389
CMD ["sh", "-c", "rm -f /var/run/xrdp/*.pid; xrdp-sesman && exec xrdp --nodaemon"]
