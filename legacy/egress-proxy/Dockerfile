FROM ubuntu/squid:latest

# Ensure runtime directories exist with correct ownership
RUN mkdir -p /var/run/squid /var/log/squid /etc/squid/acls && \
    chown -R proxy:proxy /var/run/squid /var/log/squid /etc/squid/acls

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 3128

ENTRYPOINT ["/entrypoint.sh"]
