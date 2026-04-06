FROM node:22-slim
WORKDIR /app
COPY proxy.ts report.ts package.json ./
EXPOSE 3000
CMD ["node", "--experimental-strip-types", "proxy.ts"]
