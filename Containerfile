# yoinkc tool image. Run with host root mounted at /host.
# Fedora base: Red Hat family, no subscription, works on amd64 and aarch64.
# Use :latest for current stable, or pin e.g. fedora:42 for reproducibility.
FROM fedora:latest

RUN dnf install -y python3 python3-pip systemd binutils file iproute && dnf clean all

WORKDIR /app
COPY pyproject.toml ./
COPY src/ ./src/

RUN pip install --no-cache-dir -e .

ENV YOINKC_CONTAINER=1

ENTRYPOINT ["yoinkc"]
