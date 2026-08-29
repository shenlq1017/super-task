"""SuperTask demo: FastAPI app launched by `kind: python` (entry: main.py).

SuperTask injects PORT when the service has a `port` and the environment
does not define one, so we bind to $PORT (default 8000).
"""

import os

import uvicorn
from fastapi import FastAPI

app = FastAPI()


@app.get("/health")
def health() -> dict:
    return {"status": "ok"}


@app.get("/")
def root() -> dict:
    return {"service": "python-fastapi"}


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8000"))
    uvicorn.run(app, host="127.0.0.1", port=port)
