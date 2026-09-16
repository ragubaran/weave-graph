FROM rust:1.93

# Install Python 3.10
RUN apt-get update && apt-get install -y \
    python3.10 \
    python3.10-dev \
    python3.10-distutils \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Create symbolic links for python3 and python
RUN ln -sf /usr/bin/python3.10 /usr/bin/python3 && \
    ln -sf /usr/bin/python3.10 /usr/bin/python

# Install maturin for building Python wheels
RUN pip3 install maturin

# Set the working directory
WORKDIR /workspace

# Copy the project files
COPY . .

# Install Rust targets if needed
RUN rustup target add x86_64-unknown-linux-gnu

# Set environment variables
ENV PYTHONPATH=/workspace/target/wheels