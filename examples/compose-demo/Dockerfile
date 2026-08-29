# docker.builds 条目 demo-node 的构建载体：把零依赖 Node 服务打成镜像。
# 构建需要网络拉取 node:22-alpine 基础镜像。
FROM node:22-alpine
WORKDIR /app
COPY app/server.js .
ENV PORT=3000
EXPOSE 3000
CMD ["node", "server.js"]
