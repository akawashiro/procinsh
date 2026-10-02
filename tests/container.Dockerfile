# Runtime contains Python only for the integration driver, no BPF build tools.
FROM python:3.11-slim
WORKDIR /app
COPY target/x86_64-unknown-linux-gnu/release/procinsh /usr/local/bin/procinsh
COPY scripts/dev_run.sh /app/scripts/dev_run.sh
COPY tests/container-live.py tests/sse.py /app/tests/
ENV PROCINSH_BINARY=/usr/local/bin/procinsh
CMD ["python3", "tests/container-live.py"]
