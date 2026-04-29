# syntax=docker/dockerfile:1

# ─── Builder ──────────────────────────────────────────────────────────────────
FROM oven/bun:1-alpine AS builder
WORKDIR /app

# Install deps with the committed lockfile so `--frozen-lockfile` succeeds
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

# Build the SPA
COPY . .
RUN bun run build

# ─── Runner ───────────────────────────────────────────────────────────────────
FROM nginx:alpine AS runner
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
