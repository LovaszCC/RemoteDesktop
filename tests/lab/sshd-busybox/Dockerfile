# Dropbear on a busybox userland (no find -printf, busybox tar).
FROM alpine:3.20
RUN apk add --no-cache dropbear \
 && mkdir -p /etc/dropbear \
 && adduser -D -s /bin/sh tester \
 && echo 'tester:tester-pass' | chpasswd
EXPOSE 22
CMD ["/usr/sbin/dropbear", "-F", "-E", "-R", "-p", "22"]
