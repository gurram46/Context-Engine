"""Render PDF mirror of the iQOO pitch deck with reportlab (no LibreOffice available)."""
import os
from reportlab.pdfgen.canvas import Canvas
from reportlab.lib.colors import HexColor

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "iqoo-context-engine-pitch.pdf")
W, H = 960, 540  # 13.33 x 7.5 in @72dpi

BG = HexColor("#090B10"); PANEL = HexColor("#11141C"); PANEL2 = HexColor("#151A24")
BORDER = HexColor("#262E3D"); TEXT = HexColor("#F4F6F8"); MUTED = HexColor("#9CA3AF")
ACCENT = HexColor("#22D3EE"); DIM = HexColor("#0E3A44"); GREEN = HexColor("#34D399")
AMBER = HexColor("#FBBF24"); REDC = HexColor("#F87171")

c = Canvas(OUT, pagesize=(W, H))
c.setAuthor("Team Mulanous")


def bg():
    c.setFillColor(BG); c.rect(0, 0, W, H, fill=1, stroke=0)


def box(x, y_top, w, h, fill=PANEL, border=BORDER, r=8):
    y = H - y_top - h
    c.setFillColor(fill); c.setStrokeColor(border); c.setLineWidth(1)
    c.roundRect(x, y, w, h, r, fill=1, stroke=1)


def txt(x, y_top, s, size, color=TEXT, bold=False, max_w=None, align="left"):
    c.setFillColor(color)
    c.setFont("Helvetica-Bold" if bold else "Helvetica", size)
    if align == "center":
        c.drawCentredString(x + (max_w or 0) / 2, H - y_top, s)
    else:
        if max_w:
            # simple shrink-to-fit
            while c.stringWidth(s, "Helvetica-Bold" if bold else "Helvetica", size) > max_w and size > 7:
                size -= 0.5
                c.setFont("Helvetica-Bold" if bold else "Helvetica", size)
        c.drawString(x, H - y_top, s)


def arrow(x1, y1, x2, y2, color=ACCENT, w=2):
    c.setStrokeColor(color); c.setLineWidth(w)
    c.line(x1, H - y1, x2, H - y2)


def wrap(x, y_top, s, size, color, max_w, lh=17, bold=False):
    """Word-wrap a string into multiple left-aligned lines. Returns end y."""
    c.setFillColor(color)
    words, lines, cur = s.split(" "), [], ""
    for w_ in words:
        t = (cur + " " + w_).strip()
        if c.stringWidth(t, "Helvetica-Bold" if bold else "Helvetica", size) <= max_w:
            cur = t
        else:
            if cur:
                lines.append(cur)
            cur = w_
    if cur:
        lines.append(cur)
    y = y_top
    for ln in lines:
        c.setFont("Helvetica-Bold" if bold else "Helvetica", size)
        c.drawString(x, H - y, ln)
        y += lh
    return y


def head(kicker, title, sub=None):
    txt(50, 50, kicker, 11, ACCENT, True)
    txt(50, 88, title, 27, TEXT, True)
    if sub:
        txt(50, 122, sub, 13, MUTED)
    c.setFillColor(ACCENT); c.rect(50, H - (150 if sub else 128), 65, 3, fill=1, stroke=0)


def foot(l="Context Engine  ·  Team Mulanous", r="iQOO Hackathon 2026  ·  Developer Tools"):
    txt(50, 522, l, 9, MUTED); c.setFillColor(MUTED); c.setFont("Helvetica", 9)
    c.drawRightString(W - 50, H - 522, r)


# ---- S1 ----
bg()
txt(50, 62, "iQOO HACKATHON 2026  ·  DEVELOPER TOOLS  ·  HYDERABAD  ·  TEAM MULANOUS", 11, ACCENT, True)
txt(50, 120, "Context Engine", 46, TEXT, True)
txt(50, 172, "Index once. Let every coding agent continue with the right context.", 19, TEXT)
txt(50, 200, "Local, agent-agnostic repository intelligence for AI coding agents.", 14, MUTED)
agents = ["Codex", "Claude Code", "OpenCode", "Agent X"]
for i, a in enumerate(agents):
    y = 270 + i * 56
    box(65, y, 165, 44); txt(65, y + 29, a, 12, TEXT, True, 165, "center")
    arrow(230, y + 22, 330, 352)
box(330, 298, 245, 108, DIM, ACCENT)
txt(330, 345, "CONTEXT ENGINE", 16, TEXT, True, 245, "center")
txt(330, 372, "shared local index + proofs", 11, TEXT, False, 245, "center")
box(677, 298, 210, 108, PANEL2)
txt(677, 345, "REPOSITORY", 16, TEXT, True, 210, "center")
txt(677, 372, "code + index + history", 11, MUTED, False, 210, "center")
arrow(575, 352, 677, 352, w=3)
txt(50, 452, "Several agents.   One intelligence layer.   One codebase.", 13, MUTED, False, 860, "center")
foot(); c.showPage()

# ---- S2 ----
bg(); head("THE PROBLEM", "Every new coding agent starts from zero.")
box(50, 162, 410, 187)
txt(70, 195, "AGENT A  —  learns the repo", 13, ACCENT, True)
txt(70, 228, "search  →  inspect  →  understand", 13, TEXT, True)
txt(70, 252, "symbols · dependencies · tests · architecture", 11, MUTED)
txt(70, 300, "SESSION ENDS — knowledge stays in chat history", 11, AMBER, True)
box(505, 162, 405, 187)
txt(525, 195, "AGENT B  —  repeats the work", 13, ACCENT, True)
txt(525, 228, "search again  →  inspect  →  rediscover", 13, TEXT, True)
txt(525, 252, "same files · same symbols · same questions", 11, MUTED)
txt(525, 300, "REPO MAY HAVE CHANGED — old assumptions go stale", 11, REDC, True)
arrow(460, 256, 505, 256, MUTED)
for i, (h, d) in enumerate([("Wasted context", "Repeated exploration burns context + tokens."),
                            ("Lost continuity", "Agent A's learning never transfers safely."),
                            ("Stale evidence", "Copied context rots as the repo changes.")]):
    x = 50 + i * 288
    box(x, 370, 268, 108); txt(x + 18, 405, h, 14, ACCENT, True); txt(x + 18, 432, d, 10.5, MUTED)
foot(); c.showPage()

# ---- S3 ----
bg(); head("THE SOLUTION", "One repository intelligence layer. Any coding agent.")
for i, a in enumerate(["Codex", "Claude", "OpenCode", "Agent X"]):
    y = 169 + i * 52
    box(50, y, 137, 42); txt(50, y + 28, a, 11, TEXT, True, 137, "center")
    arrow(187, y + 21, 259, 281)
box(259, 216, 238, 130, DIM, ACCENT)
txt(259, 264, "CONTEXT ENGINE", 15, TEXT, True, 238, "center")
txt(259, 290, "index once · serve many", 11, TEXT, False, 238, "center")
box(590, 216, 320, 130, PANEL2)
txt(590, 264, "REPOSITORY", 15, TEXT, True, 320, "center")
txt(590, 290, "indexed · verified state", 11, MUTED, False, 320, "center")
arrow(497, 281, 590, 281, w=3)
for i, (h, d) in enumerate([("STRUCTURAL", "tree-sitter · symbols · deps"), ("LEXICAL", "BM25 · exact retrieval"),
                            ("SEMANTIC", "embedding retrieval"), ("PACKING", "compact evidence ctx"),
                            ("MCP", "agent-neutral interface")]):
    x = 50 + i * 176
    box(x, 378, 161, 72); txt(x, 405, h, 10.5, ACCENT, True, 161, "center"); txt(x, 426, d, 9, MUTED, False, 161, "center")
txt(50, 478, "Context Engine indexes the repository once and serves focused evidence to compatible agents.", 12.5, TEXT, False, 860, "center")
foot(); c.showPage()

# ---- S4 ----
bg(); head("CORE DIFFERENTIATOR", "Search is only step one. Continuity is the real problem.")
blocks = [("1 · CONTEXT PROOF", '"What did Agent A see?"', "Hashes of evidence + repo state.", "✓  BUILT", ACCENT, DIM),
          ("2 · CONTEXT DELTA", '"Is that evidence still valid?"', None, "✓  BUILT", GREEN, PANEL),
          ("3 · CROSS-AGENT HANDOFF", '"What should Agent B use now?"', "Delta + fresh ctx + new proof.", "◆  HACKATHON BUILD", AMBER, DIM)]
for i, (h, q, d, tag, col, fl) in enumerate(blocks):
    x = 50 + i * 288
    box(x, 162, 268, 137, fl, col)
    txt(x + 18, 195, h, 12, col, True); txt(x + 18, 220, q, 11, TEXT)
    if i == 1:
        txt(x + 18, 245, "UNCHANGED / CHANGED", 10, MUTED); txt(x + 18, 262, "REMOVED / UNVERIFIABLE", 10, MUTED)
    else:
        txt(x + 18, 245, d, 10, MUTED)
    txt(x + 18, 278, tag, 10, col, True)
flow = ["Agent A", "Context", "PROOF A", "repo Δ", "DELTA", "Agent B", "fresh ctx", "PROOF B"]
for i, f in enumerate(flow):
    x = 50 + i * 109
    hot = f in ("PROOF A", "DELTA", "PROOF B")
    box(x, 324, 92, 45, DIM if hot else PANEL, ACCENT if hot else BORDER, r=6)
    txt(x, 353, f, 10, TEXT if hot else MUTED, True, 92, "center")
    if i < 7:
        arrow(x + 92, 346, x + 109, 346, w=1.5)
box(50, 392, 440, 90, PANEL2); txt(50, 448, "No chat-history replay required.", 14, TEXT, True, 440, "center")
box(510, 392, 400, 90, PANEL2); txt(510, 448, "Handoff reference is tiny; intelligence stays local.", 12, MUTED, False, 400, "center")
foot(); c.showPage()

# ---- S5 ----
bg(); head("LIVE DEMO", "What the demo looks like  —  retry.rs · unrelated.rs")
steps = [("STEP 1 · Agent A asks", '"Where is retry policy enforced?"', "Returns evidence + Proof A.", True),
         ("STEP 2 · Dev edits unrelated.rs", "Repository changes elsewhere.", "Retry evidence unaffected.", False),
         ("STEP 3 · Agent B starts fresh", "Only gets: query + Proof A.", "No chat history transferred.", True),
         ("STEP 4 · Engine answers", "UNCHANGED → fresh ctx → Proof B.", "Continuity without replay.", True)]
for i, (h, q, d, hot) in enumerate(steps):
    x = 50 + i * 216
    box(x, 162, 201, 166, DIM if hot else PANEL, ACCENT if hot else BORDER)
    txt(x + 14, 195, h, 10.5, ACCENT if hot else MUTED, True)
    y_end = wrap(x + 14, 222, q, 10.5, TEXT, 173)
    txt(x + 14, y_end + 12, d, 10, MUTED)
    if i < 3:
        arrow(x + 201, 245, x + 216, 245, w=1.5)
box(50, 349, 860, 133, PANEL2)
txt(50, 392, "THEN: dev modifies retry.rs  →  Agent C continues from Proof B  →  Delta: CHANGED  →  fresh context returned.", 12, TEXT, False, 860, "center")
txt(50, 420, "Same workflow proves both paths: untouched evidence stays valid, edited evidence is caught.", 11, MUTED, False, 860, "center")
foot(); c.showPage()

# ---- S6 ----
bg(); head("WHY IT MATTERS", "Why this matters when developers use multiple AI agents.")
for i, (h, d) in enumerate([("LESS REDISCOVERY", "One shared repo intelligence layer."),
                            ("SAFER CONTINUITY", "Old evidence checked before reuse."),
                            ("AGENT INDEPENDENCE", "Switch agents without moving chats.")]):
    x = 50 + i * 288
    box(x, 169, 268, 126, DIM if i == 1 else PANEL, ACCENT if i == 1 else BORDER)
    txt(x + 18, 205, h, 13, ACCENT, True); txt(x + 18, 232, d, 11, TEXT)
for i, (h, d) in enumerate([("Local-first", "intelligence stays on machine"), ("Deterministic", "Proof → Delta → Handoff"),
                            ("Shared runtime", "one resource-managed daemon"), ("Conservative", "UNVERIFIABLE > guessing")]):
    x = 50 + i * 216
    box(x, 328, 201, 68); txt(x, 355, h, 11, ACCENT, True, 201, "center"); txt(x, 374, d, 8.5, MUTED, False, 201, "center")
txt(50, 452, "Search finds code. Context Engine makes repo context survive safely across agent sessions.", 12.5, TEXT, False, 860, "center")
foot(); c.showPage()

# ---- S7 ----
bg(); head("HACKATHON PLAN", "What we will deliver at iQOO Hackathon 2026")
box(50, 162, 266, 216, PANEL, GREEN)
txt(68, 195, "ALREADY BUILT", 11, GREEN, True)
for i, t in enumerate(["structural repo indexing", "hybrid retrieval", "shared daemon + MCP", "Context Proof (merged)", "Context Delta (merged)"]):
    txt(68, 222 + i * 30, "✓  " + t, 10, TEXT)
box(339, 162, 295, 216, DIM, ACCENT)
txt(357, 195, "HACKATHON BUILD", 11, ACCENT, True)
for i, t in enumerate(["Cross-Agent Handoff", "Agent A → B workflow", "live MCP demo", "controlled A/B evaluation:"]):
    txt(357, 222 + i * 27, "→  " + t, 10, TEXT)
for i, t in enumerate(["task completion · repo exploration", "tool calls · context usage"]):
    txt(375, 330 + i * 24, t, 9.5, MUTED)
box(655, 162, 255, 216, PANEL2)
txt(673, 195, "TEAM MULANOUS", 11, TEXT, True)
txt(673, 220, "Sandeep Gurram", 12, ACCENT, True)
txt(673, 242, "mobile/web systems · backend", 9.5, MUTED)
txt(673, 258, "Rust / Go / TS · GCP / Postgres", 9.5, MUTED)
txt(673, 274, "retrieval infra · AI-agent workflows", 9.5, MUTED)
txt(673, 290, "Google hackathon participant", 9.5, MUTED)
box(50, 392, 860, 72, PANEL, AMBER, r=6)
txt(50, 410, "HACKATHON SCOPE", 10, AMBER, True, 860, "center")
txt(50, 431, "Pre-existing:   indexing · hybrid retrieval · shared daemon · Context Proof · Context Delta", 10, TEXT, False, 860, "center")
txt(50, 449, "Built at iQOO:   Cross-Agent Handoff · demo integration · A/B evaluation", 10, TEXT, False, 860, "center")
txt(50, 492, "One codebase. Many agents. Continuous context.", 17, TEXT, True, 860, "center")
foot(); c.showPage()

c.save()
print("saved", OUT)
