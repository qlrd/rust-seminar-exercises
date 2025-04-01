FROM ubuntu:22.04

ARG SEEDER_HOST
ARG SEEDER_NAMESERVER
ARG SEEDER_MAILBOX
ARG SEEDER_IP="::"
ARG SEEDER_PORT=15353

ENV DEBIAN_FRONTEND=noninteractive \
  BUILD_DIR="/build" \
  SEEDER_HOST=${SEEDER_HOST} \
  SEEDER_NAMESERVER=${SEEDER_NAMESERVER} \
  SEEDER_MAILBOX=${SEEDER_MAILBOX} \
  SEEDER_IP=${SEEDER_IP} \
  SEEDER_PORT=${SEEDER_PORT}

RUN apt-get update && apt-get install -y \
  build-essential \
  libboost-all-dev \
  libssl-dev \
  git \
  logrotate && \
  rm -rf /var/lib/apt/lists/*

RUN git clone https://github.com/sipa/bitcoin-seeder.git ${BUILD_DIR}

WORKDIR ${BUILD_DIR}

RUN make && \
  cp ./dnsseed /usr/local/bin && \
  rm -rf ${BUILD_DIR}

# Setup log rotation
RUN mkdir -p /etc/logrotate.d /logs && \
  cat <<EOF > /etc/logrotate.d/dnsseed
/logs/dnsseed.log {
  daily
  rotate 7
  compress
  delaycompress
  missingok
  notifempty
  copytruncate
}
EOF

WORKDIR /

CMD ["/bin/bash", "-c", \
  "(while true; do logrotate -f /etc/logrotate.d/dnsseed; sleep 86400; done) & \
  dnsseed -h \"$SEEDER_HOST\" -n \"$SEEDER_NAMESERVER\" -m \"$SEEDER_MAILBOX\" -i \"$SEEDER_IP\" -p \"$SEEDER_PORT\" >> /logs/dnsseed.log 2>&1"]
