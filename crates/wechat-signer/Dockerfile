# wechat-signer 镜像：静态链接（musl）+ scratch，镜像约 6 MB。
# 在仓库根目录构建：docker build -f crates/wechat-signer/Dockerfile -t wechat-signer .

FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev gcc
WORKDIR /src
COPY . .
RUN cargo build -p wechat-signer --release --locked \
    && cp target/release/wechat-signer /wechat-signer

FROM scratch
# reqwest 使用 rustls + 内置根证书，不需要系统 CA
COPY --from=build /wechat-signer /wechat-signer
USER 65534:65534
ENV WECHAT_LISTEN=0.0.0.0:8080
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD ["/wechat-signer", "health"]
ENTRYPOINT ["/wechat-signer"]
