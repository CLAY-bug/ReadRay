from __future__ import annotations

import re
import zipfile
from pathlib import Path
from xml.etree import ElementTree

import requests
from bs4 import BeautifulSoup
from urllib3 import disable_warnings
from urllib3.exceptions import InsecureRequestWarning


disable_warnings(InsecureRequestWarning)

ROOT = Path(__file__).resolve().parents[1]
RESOURCE_DIR = ROOT / "resource"

PAGES = [
    {
        "url": "https://cpipc.acge.org.cn/cw/hp/2c9088a5696cbf370169a3f8101510bd",
        "path": "official_2026_homepage.md",
        "title": "第八届中国研究生人工智能创新大赛主页",
    },
    {
        "url": "https://cpipc.acge.org.cn/cw/contestNews/detail/2c9088a5696cbf370169a3f8101510bd/2c9080179e403028019e496a8feb14b9?page=0",
        "path": "official_2026_topics_and_submission.md",
        "title": "第八届中国研究生人工智能创新大赛赛题",
    },
    {
        "url": "https://cpipc.acge.org.cn/cw/contestNews/detail/2c9088a5696cbf370169a3f8101510bd/2c9080179e403028019e4963ef9f149e?page=0",
        "path": "official_2026_invitation_and_rules.md",
        "title": "第八届中国研究生人工智能创新大赛参赛邀请函",
    },
]

ATTACHMENTS = [
    {
        "url": "https://cpipc.acge.org.cn/sysFile/downFile.do?fileId=eb97c2296ac442b8adf5a3b869acbb91",
        "path": "attachment1_initial_submission_spec.pdf",
    },
    {
        "url": "https://cpipc.acge.org.cn/sysFile/downFile.do?fileId=3bc820b99fbd467bb1cacab9cf975291",
        "path": "attachment2_project_document_template.pdf",
    },
    {
        "url": "https://cpipc.acge.org.cn/sysFile/downFile.do?fileId=5905f989fc5b40eb96d54fd964d4ecaa",
        "path": "attachment3_huawei_topics.docx",
    },
    {
        "url": "https://cpipc.acge.org.cn/sysFile/downFile.do?fileId=075ef6b1934e4903914d706e6356da11",
        "path": "invitation_attachment1_participant_guide.pdf",
    },
    {
        "url": "https://cpipc.acge.org.cn/sysFile/downFile.do?fileId=7b270eefd9854bde8e6147b844171eb5",
        "path": "invitation_attachment2_invitation_letter.pdf",
    },
]


def normalize_text(text: str) -> str:
    lines = []
    for line in text.splitlines():
        line = re.sub(r"\s+", " ", line).strip()
        if line:
            lines.append(line)
    return "\n".join(lines)


def fetch(url: str) -> requests.Response:
    response = requests.get(url, timeout=60, verify=False)
    response.raise_for_status()
    return response


def write_page(page: dict[str, str]) -> None:
    response = fetch(page["url"])
    response.encoding = response.encoding or "utf-8"
    soup = BeautifulSoup(response.text, "html.parser")
    for tag in soup(["script", "style", "noscript"]):
        tag.decompose()
    text = normalize_text(soup.get_text("\n"))
    content = (
        f"# {page['title']}\n\n"
        f"来源：{page['url']}\n\n"
        "```text\n"
        f"{text}\n"
        "```\n"
    )
    (RESOURCE_DIR / page["path"]).write_text(content, encoding="utf-8")


def download_attachment(item: dict[str, str]) -> None:
    response = fetch(item["url"])
    (RESOURCE_DIR / item["path"]).write_bytes(response.content)


def extract_pdf(path: Path) -> str:
    import fitz

    doc = fitz.open(path)
    pages = []
    for index, page in enumerate(doc, start=1):
        page_text = page.get_text("text").strip()
        pages.append(f"## 第 {index} 页\n\n{page_text}")
    return "\n\n".join(pages).strip() + "\n"


def extract_docx(path: Path) -> str:
    ns = {"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}
    with zipfile.ZipFile(path) as archive:
        xml = archive.read("word/document.xml")
    root = ElementTree.fromstring(xml)
    paragraphs = []
    for paragraph in root.findall(".//w:p", ns):
        parts = [node.text for node in paragraph.findall(".//w:t", ns) if node.text]
        line = "".join(parts).strip()
        if line:
            paragraphs.append(line)
    return "\n".join(paragraphs).strip() + "\n"


def extract_attachment_text(path: Path) -> None:
    suffix = path.suffix.lower()
    if suffix == ".pdf":
        text = extract_pdf(path)
    elif suffix == ".docx":
        text = extract_docx(path)
    else:
        return
    path.with_suffix(".txt").write_text(text, encoding="utf-8")


def main() -> None:
    RESOURCE_DIR.mkdir(exist_ok=True)
    for page in PAGES:
        write_page(page)
    for attachment in ATTACHMENTS:
        file_path = RESOURCE_DIR / attachment["path"]
        download_attachment(attachment)
        extract_attachment_text(file_path)
    print("resource restored")


if __name__ == "__main__":
    main()
