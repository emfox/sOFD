# syntax=docker/dockerfile:1
#
# The server, as an image. The desktop reader needs a display and is not here:
# `sofd-server` is the form a deployment runs, and it exposes the same rendering
# and verification over HTTP.
#
# The binary is built outside this file, by `release.yml`, and copied in. That
# is deliberate: the workflow already builds it on a runner with the toolchain,
# the caches and the C library the build needs, and a second build inside the
# image would be a second answer to "what was released".

FROM debian:bookworm-slim

# The distribution's own archive is the default. A build behind a network that
# cannot reach it passes another one: `--build-arg APT_MIRROR=mirror.example`.
ARG APT_MIRROR=""
RUN if [ -n "$APT_MIRROR" ]; then \
        sed -i "s|http://deb.debian.org|$APT_MIRROR|g" /etc/apt/sources.list.d/debian.sources; \
    fi

# What the binary links, and nothing else. Each one is a `ldd` line: the fonts
# are reached through fontconfig, which is what a document without embedded
# fonts is drawn with, and the rest are the image and archive codecs.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libfontconfig1 \
        libfreetype6 \
        libexpat1 \
        libpng16-16 \
        libbz2-1.0 \
        libbrotli1 \
        fonts-wqy-zenhei \
    && rm -rf /var/lib/apt/lists/*

# Not root: the service reads uploaded documents, and a parser bug should not
# leave a shell in the container. The uid is fixed so a mounted data directory
# keeps the same owner across upgrades.
RUN groupadd --gid 10001 sofd \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin sofd

COPY sofd-server /usr/local/bin/sofd-server

# Documents arrive over the network, so the store is in memory unless a
# deployment mounts a directory and names it.
VOLUME /var/lib/sofd
RUN mkdir -p /var/lib/sofd && chown sofd:sofd /var/lib/sofd

USER sofd
ENV SOFD_BIND=0.0.0.0:8080 \
    SOFD_DATA_DIR=/var/lib/sofd
EXPOSE 8080

# The route the service already answers, so a container that started but cannot
# serve is not reported healthy.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["sofd-server", "--health-check"]

ENTRYPOINT ["sofd-server"]
