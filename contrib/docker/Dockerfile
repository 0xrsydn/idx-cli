# syntax=docker/dockerfile:1
#
# Images for ghcr.io/0xrsydn/idx-cli and ghcr.io/0xrsydn/idx-mcp, built by
# .github/workflows/images.yml from a release's static musl binaries. The
# context holds dist/linux-<arch>/{idx,idx-mcp}; nothing is compiled here, so
# a multi-platform build needs no emulation.
#
# The base is distroless static: CA certificates and a nonroot user whose home
# (/home/nonroot) holds the cache, config and ownership database. It has no
# shell, curl-impersonate or MuPDF, so the commands that need those helper
# tools (see the install docs) don't work in the image.

FROM gcr.io/distroless/static-debian12:nonroot AS base
LABEL org.opencontainers.image.licenses="MIT"

FROM base AS idx-cli
ARG TARGETARCH
COPY --chmod=0755 dist/linux-${TARGETARCH}/idx /usr/local/bin/idx
ENTRYPOINT ["/usr/local/bin/idx"]

FROM base AS idx-mcp
ARG TARGETARCH
COPY --chmod=0755 dist/linux-${TARGETARCH}/idx-mcp /usr/local/bin/idx-mcp
ENTRYPOINT ["/usr/local/bin/idx-mcp"]
