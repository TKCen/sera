# syntax=docker/dockerfile:1

# ─── Base stage (used by dev compose via `target: base`) ──────────────────────
FROM node:20-alpine AS base
WORKDIR /app
COPY package*.json ./
RUN npm ci --ignore-scripts

# ─── Builder (used by production builds) ─────────────────────────────────────
FROM base AS builder
COPY . .
RUN npm run build

# ─── Production runner ────────────────────────────────────────────────────────
FROM nginx:alpine AS runner
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
