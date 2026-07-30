"""
SEC 10-K Multitenant RAG — QQL Edition. One source of truth.
"""

from __future__ import annotations

import os
import re

QDRANT_URL = os.getenv("QDRANT_URL", "http://localhost:6333")
# OpenAI-compatible embeddings endpoint (LM Studio, Ollama, vLLM, …)
LM_STUDIO = os.getenv("EMBED_URL", "http://127.0.0.1:1234")
EMBED_MODEL = os.getenv("EMBED_MODEL", "text-embedding-all-minilm-l6-v2-embedding")
EMBED_DIM = int(os.getenv("EMBED_DIM", "384"))
LLM_MODEL = os.getenv("LLM_MODEL", "minicpm5-1b-claude-opus-fable5-v2-thinking")

COLLECTION = "sec10k"
TENANTS = ["honeywell", "ge", "3m", "rtx"]

CHUNK_SIZE = 384
CHUNK_OVERLAP = 50

SEC_USER_AGENT = os.getenv("SEC_USER_AGENT", "QQL-demo contact@example.com")

# Real SEC EDGAR 10-K filings (company → fiscal year → filing URL)
FILINGS = {
    "honeywell": {
        2023: "https://www.sec.gov/Archives/edgar/data/773840/000077384024000014/hon-20231231.htm",
        2024: "https://www.sec.gov/Archives/edgar/data/773840/000077384025000010/hon-20241231.htm",
        2025: "https://www.sec.gov/Archives/edgar/data/773840/000077384026000013/hon-20251231.htm",
    },
    "ge": {
        2023: "https://www.sec.gov/Archives/edgar/data/40545/000004054524000027/ge-20231231.htm",
        2024: "https://www.sec.gov/Archives/edgar/data/40545/000004054525000015/ge-20241231.htm",
        2025: "https://www.sec.gov/Archives/edgar/data/40545/000004054526000008/ge-20251231.htm",
    },
    "3m": {
        2023: "https://www.sec.gov/Archives/edgar/data/66740/000006674024000016/mmm-20231231.htm",
        2024: "https://www.sec.gov/Archives/edgar/data/66740/000006674025000006/mmm-20241231.htm",
        2025: "https://www.sec.gov/Archives/edgar/data/66740/000006674026000014/mmm-20251231.htm",
    },
    "rtx": {
        2023: "https://www.sec.gov/Archives/edgar/data/101829/000010182924000008/rtx-20231231.htm",
        2024: "https://www.sec.gov/Archives/edgar/data/101829/000010182925000005/rtx-20241231.htm",
        2025: "https://www.sec.gov/Archives/edgar/data/101829/000010182926000006/rtx-20251231.htm",
    },
}

# Metadata extraction — stored as payload for WHERE / GROUP BY / ORDER BY
SECTION_RE = re.compile(r"(Item\s+[0-9]+[A-Z]?\.?)", re.IGNORECASE)
RISK_RE = re.compile(
    r"(cyber\s*security|supply\s*chain|regulatory|litigation|"
    r"economic\s*condition|competition|intellectual\s*property|"
    r"environmental|geopolitical|inflation|foreign\s*exchange|"
    r"pandemic|data\s*privacy|trade\s+restriction)",
    re.IGNORECASE,
)
REVENUE_RE = re.compile(r"\$[\d,]+\.?\d*\s*(million|billion|trillion)", re.IGNORECASE)
