FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        pdns-server \
        pdns-backend-sqlite3 \
        sqlite3 \
    && rm -rf /var/lib/apt/lists/*

COPY docker/pdns48/pdns.conf /etc/powerdns/pdns.conf
COPY docker/pdns48/entrypoint.sh /usr/local/bin/pdns48-entrypoint.sh

RUN chmod +x /usr/local/bin/pdns48-entrypoint.sh

EXPOSE 5300/tcp 5300/udp 8081/tcp

ENTRYPOINT ["/usr/local/bin/pdns48-entrypoint.sh"]
