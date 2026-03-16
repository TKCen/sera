FROM node:20-slim

WORKDIR /app

RUN apt-get update && apt-get install -y curl && rm -rf /var/lib/apt/lists/*

COPY package*.json ./
RUN npm install

COPY . .

RUN npm run build && npm install --omit=dev

EXPOSE 3001

HEALTHCHECK CMD curl -f http://localhost:3001/api/health || exit 1

CMD ["npm", "start"]
