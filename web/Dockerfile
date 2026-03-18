FROM node:20-alpine AS base
WORKDIR /app
RUN apk add --no-cache curl
COPY package*.json ./
RUN npm install

FROM base AS builder
COPY . .
ARG CORE_API_URL=http://sera-core:3001
ENV CORE_API_URL=${CORE_API_URL}
RUN npm run build

FROM node:20-alpine AS runner
WORKDIR /app
RUN apk add --no-cache curl
ENV NODE_ENV=production
COPY --from=builder /app/public ./public
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
EXPOSE 3000
HEALTHCHECK CMD curl -f http://localhost:3000/ || exit 1
CMD ["node", "server.js"]
