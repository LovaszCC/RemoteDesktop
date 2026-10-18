# OpenSSH on a GNU userland (find -printf, GNU tar).
FROM debian:12-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-server tar gzip \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/sshd \
 && useradd -m -s /bin/bash tester \
 && echo 'tester:tester-pass' | chpasswd \
 && echo 'root:root-pass' | chpasswd \
 && sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config \
 && sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
