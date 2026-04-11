FROM alpine:3.19
RUN apk add --no-cache curl jq socat
COPY agent.sh /app/agent.sh
RUN chmod +x /app/agent.sh
WORKDIR /workspace
ENTRYPOINT ["/app/agent.sh"]
