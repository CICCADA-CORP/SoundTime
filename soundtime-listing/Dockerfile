FROM node:20-alpine

WORKDIR /app

COPY package.json package-lock.json* ./
RUN npm ci --production

COPY . .

RUN mkdir -p /app/data

ENV PORT=3333
ENV DB_PATH=/app/data/listing.db

EXPOSE 3333

CMD ["node", "server.js"]
