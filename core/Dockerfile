FROM node:20-slim AS base
WORKDIR /app
RUN apt-get update && apt-get install -y curl && rm -rf /var/lib/apt/lists/*
COPY package*.json ./
RUN npm install

FROM base AS builder
COPY . .
RUN npm run build

FROM base AS runner
COPY --from=builder /app/dist ./dist
# Re-install only production dependencies
RUN npm prune --omit=dev
EXPOSE 3001
HEALTHCHECK CMD curl -f http://localhost:3001/api/health || exit 1
CMD ["node", "dist/index.js"]
