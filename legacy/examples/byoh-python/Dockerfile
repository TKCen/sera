FROM python:3.12-slim
RUN pip install --no-cache-dir requests
COPY agent.py /app/agent.py
WORKDIR /workspace
ENTRYPOINT ["python3", "/app/agent.py"]
