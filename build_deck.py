"""Build iQOO Hackathon 2026 pitch deck: PPTX (python-pptx) + PDF (reportlab)."""
import os
from pptx import Presentation
from pptx.util import Inches, Pt
from pptx.dml.color import RGBColor
from pptx.enum.text import PP_ALIGN, MSO_ANCHOR
from pptx.enum.shapes import MSO_SHAPE, MSO_CONNECTOR

OUT_DIR = os.path.dirname(os.path.abspath(__file__))
PPTX_PATH = os.path.join(OUT_DIR, "iqoo-context-engine-pitch.pptx")

# Palette
BG = RGBColor(0x09, 0x0B, 0x10)
PANEL = RGBColor(0x11, 0x14, 0x1C)
PANEL2 = RGBColor(0x15, 0x1A, 0x24)
BORDER = RGBColor(0x26, 0x2E, 0x3D)
TEXT = RGBColor(0xF4, 0xF6, 0xF8)
MUTED = RGBColor(0x9C, 0xA3, 0xAF)
ACCENT = RGBColor(0x22, 0xD3, 0xEE)
ACCENT_DIM = RGBColor(0x0E, 0x3A, 0x44)
GREEN = RGBColor(0x34, 0xD3, 0x99)
AMBER = RGBColor(0xFB, 0xBF, 0x24)
RED = RGBColor(0xF8, 0x71, 0x71)

FONT = "Calibri"
SW, SH = Inches(13.333), Inches(7.5)

prs = Presentation()
prs.slide_width = SW
prs.slide_height = SH
BLANK = prs.slide_layouts[6]


def new_slide():
    s = prs.slides.add_slide(BLANK)
    s.background.fill.solid()
    s.background.fill.fore_color.rgb = BG
    return s


def tb(slide, l, t, w, h):
    return slide.shapes.add_textbox(Inches(l), Inches(t), Inches(w), Inches(h))


def para(tf, text, size, color=TEXT, bold=False, align=PP_ALIGN.LEFT, space_after=Pt(2), first=False):
    p = tf.paragraphs[0] if first else tf.add_paragraph()
    p.alignment = align
    p.space_after = space_after
    p.space_before = Pt(0)
    p.line_spacing = 1.05
    r = p.add_run()
    r.text = text
    r.font.size = Pt(size)
    r.font.bold = bold
    r.font.color.rgb = color
    r.font.name = FONT
    return p


def set_text(shape, lines, anchor=MSO_ANCHOR.MIDDLE):
    tf = shape.text_frame
    tf.word_wrap = True
    tf.auto_size = None
    tf.vertical_anchor = anchor
    first = True
    for text, size, color, bold, align in lines:
        para(tf, text, size, color, bold, align, first=first)
        first = False
    return tf


def box(slide, l, t, w, h, fill=PANEL, border=BORDER, radius=True):
    shp = slide.shapes.add_shape(
        MSO_SHAPE.ROUNDED_RECTANGLE if radius else MSO_SHAPE.RECTANGLE,
        Inches(l), Inches(t), Inches(w), Inches(h),
    )
    shp.fill.solid()
    shp.fill.fore_color.rgb = fill
    shp.line.color.rgb = border
    shp.line.width = Pt(1)
    shp.shadow.inherit = False
    return shp


def accent_box(slide, l, t, w, h):
    shp = box(slide, l, t, w, h, fill=ACCENT_DIM, border=ACCENT)
    return shp


def arrow(slide, l1, t1, l2, t2, color=ACCENT, width=2.0):
    c = slide.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(l1), Inches(t1), Inches(l2), Inches(t2))
    c.line.color.rgb = color
    c.line.width = Pt(width)
    return c


def label(slide, l, t, w, h, text, size=13, color=MUTED, bold=False, align=PP_ALIGN.LEFT):
    b = tb(slide, l, t, w, h)
    tf = b.text_frame
    tf.word_wrap = True
    para(tf, text, size, color, bold, align, first=True)
    return b


def slide_head(slide, kicker, title, sub=None):
    label(slide, 0.7, 0.35, 11.9, 0.35, kicker, 13, ACCENT, True)
    label(slide, 0.7, 0.68, 11.9, 0.75, title, 32, TEXT, True)
    if sub:
        label(slide, 0.7, 1.42, 11.9, 0.45, sub, 16, MUTED, False)
    # thin accent rule
    rule = slide.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(0.7), Inches(1.95 if sub else 1.55), Inches(0.9), Pt(3))
    rule.fill.solid(); rule.fill.fore_color.rgb = ACCENT
    rule.line.fill.background()
    rule.shadow.inherit = False


def footer(slide, left="Context Engine  ·  Team Mulanous", right="iQOO Hackathon 2026  ·  Developer Tools"):
    label(slide, 0.7, 7.0, 6.0, 0.3, left, 11, MUTED)
    label(slide, 6.6, 7.0, 6.0, 0.3, right, 11, MUTED, align=PP_ALIGN.RIGHT)


# ============ SLIDE 1 — TITLE ============
s = new_slide()
label(s, 0.7, 0.55, 11.9, 0.4, "iQOO HACKATHON 2026  ·  DEVELOPER TOOLS  ·  HYDERABAD  ·  TEAM MULANOUS", 13, ACCENT, True)
b = tb(s, 0.7, 1.0, 11.9, 1.3)
tf = b.text_frame; tf.word_wrap = True
para(tf, "Context Engine", 54, TEXT, True, first=True)
b2 = tb(s, 0.7, 2.25, 11.9, 0.7)
tf = b2.text_frame; tf.word_wrap = True
para(tf, "Index once. Let every coding agent continue with the right context.", 24, TEXT, False, first=True)
label(s, 0.7, 2.95, 11.9, 0.4, "Local, agent-agnostic repository intelligence for AI coding agents.", 17, MUTED)

# diagram: 4 agents -> engine -> repo
agents = ["Codex", "Claude Code", "OpenCode", "Agent X"]
for i, name in enumerate(agents):
    y = 3.75 + i * 0.78
    shp = box(s, 0.9, y, 2.3, 0.62)
    set_text(shp, [(name, 15, TEXT, True, PP_ALIGN.CENTER)])
    arrow(s, 3.2, y + 0.31, 4.6, 4.9)
eng = accent_box(s, 4.6, 4.15, 3.4, 1.5)
set_text(eng, [("CONTEXT ENGINE", 20, TEXT, True, PP_ALIGN.CENTER),
               ("shared local index + proofs", 13, TEXT, False, PP_ALIGN.CENTER)])
repo = box(s, 9.4, 4.15, 2.9, 1.5, fill=PANEL2)
set_text(repo, [("REPOSITORY", 20, TEXT, True, PP_ALIGN.CENTER),
                ("code + index + history", 13, MUTED, False, PP_ALIGN.CENTER)])
arrow(s, 8.0, 4.9, 9.4, 4.9, width=2.5)
label(s, 0.9, 6.15, 11.5, 0.4, "Several agents.  One intelligence layer.  One codebase.", 15, MUTED, align=PP_ALIGN.CENTER)
footer(s)
s.notes_slide.placeholders[1].text_frame.text = (
    "Context Engine is a local repository intelligence layer for AI coding agents. "
    "Index the repo once, and let every coding agent continue with the right context."
)

# ============ SLIDE 2 — PROBLEM ============
s = new_slide()
slide_head(s, "THE PROBLEM", "Every new coding agent starts from zero.")
# Agent A panel
pa = box(s, 0.7, 2.25, 5.7, 2.6)
set_text(pa, [("AGENT A  —  learns the repo", 16, ACCENT, True, PP_ALIGN.LEFT)], anchor=MSO_ANCHOR.TOP)
for i, step in enumerate(["search  →  inspect  →  understand", "symbols · dependencies · tests · architecture"]):
    label(s, 1.0, 3.15 + i * 0.45, 5.1, 0.4, step, 15 if i == 0 else 13, TEXT if i == 0 else MUTED)
label(s, 1.0, 4.15, 5.1, 0.4, "SESSION ENDS  —  knowledge stays in chat history", 13, AMBER, True)
# Agent B panel
pb = box(s, 7.0, 2.25, 5.6, 2.6)
set_text(pb, [("AGENT B  —  repeats the work", 16, ACCENT, True, PP_ALIGN.LEFT)], anchor=MSO_ANCHOR.TOP)
for i, step in enumerate(["search again  →  inspect again  →  rediscover", "same files · same symbols · same questions"]):
    label(s, 7.3, 3.15 + i * 0.45, 5.0, 0.4, step, 15 if i == 0 else 13, TEXT if i == 0 else MUTED)
label(s, 7.3, 4.15, 5.0, 0.4, "REPO MAY HAVE CHANGED  —  old assumptions go stale", 13, RED, True)
arrow(s, 6.4, 3.55, 7.0, 3.55, color=MUTED)
# consequences
cons = [("Wasted context", "Repeated exploration burns context window + tokens.", ACCENT),
        ("Lost continuity", "Agent A's learning never transfers safely to Agent B.", ACCENT),
        ("Stale evidence", "Copied context silently rots as the repo changes.", ACCENT)]
for i, (h, d, c) in enumerate(cons):
    x = 0.7 + i * 4.0
    shp = box(s, x, 5.15, 3.7, 1.5)
    set_text(shp, [(h, 17, c, True, PP_ALIGN.LEFT), (d, 13.5, MUTED, False, PP_ALIGN.LEFT)], anchor=MSO_ANCHOR.MIDDLE)
footer(s)
s.notes_slide.placeholders[1].text_frame.text = (
    "Every new agent session starts from zero: it re-explores the repo, burns context, "
    "loses what the last agent learned, and risks trusting stale code. No invented stats — this is the daily pain."
)

# ============ SLIDE 3 — SOLUTION ============
s = new_slide()
slide_head(s, "THE SOLUTION", "One repository intelligence layer. Any coding agent.")
# hub diagram
hub_agents = ["Codex", "Claude", "OpenCode", "Agent X"]
for i, name in enumerate(hub_agents):
    y = 2.35 + i * 0.72
    shp = box(s, 0.7, y, 1.9, 0.58)
    set_text(shp, [(name, 14, TEXT, True, PP_ALIGN.CENTER)])
    arrow(s, 2.6, y + 0.29, 3.6, 3.9)
eng = accent_box(s, 3.6, 3.0, 3.3, 1.8)
set_text(eng, [("CONTEXT ENGINE", 19, TEXT, True, PP_ALIGN.CENTER),
               ("index once · serve many", 13, TEXT, False, PP_ALIGN.CENTER)])
repo = box(s, 8.2, 3.0, 3.4, 1.8, fill=PANEL2)
set_text(repo, [("REPOSITORY", 19, TEXT, True, PP_ALIGN.CENTER),
                ("indexed · verified state", 13, MUTED, False, PP_ALIGN.CENTER)])
arrow(s, 6.9, 3.9, 8.2, 3.9, width=2.5)
# layer pills
layers = [("STRUCTURAL", "tree-sitter · symbols · deps"),
          ("LEXICAL", "BM25 · exact retrieval"),
          ("SEMANTIC", "embedding retrieval"),
          ("PACKING", "compact evidence context"),
          ("MCP", "agent-neutral interface")]
for i, (h, d) in enumerate(layers):
    x = 0.7 + i * 2.44
    shp = box(s, x, 5.25, 2.24, 1.0)
    set_text(shp, [(h, 13.5, ACCENT, True, PP_ALIGN.CENTER), (d, 11.5, MUTED, False, PP_ALIGN.CENTER)])
label(s, 0.7, 6.4, 11.9, 0.4,
      "Context Engine indexes the repository once and serves focused, evidence-backed context to compatible agents.", 15, TEXT, align=PP_ALIGN.CENTER)
footer(s)
s.notes_slide.placeholders[1].text_frame.text = (
    "Instead of every agent indexing the world, Context Engine indexes once — structural, lexical, semantic — "
    "packs compact evidence, and serves it over an agent-neutral MCP interface."
)

# ============ SLIDE 4 — DIFFERENTIATOR ============
s = new_slide()
slide_head(s, "CORE DIFFERENTIATOR", "Search is only step one. Continuity is the real problem.")
blocks = [
    ("1 · CONTEXT PROOF", '"What exactly did Agent A see?"',
     "Hashes of delivered evidence + indexed repo state.", "✓  BUILT", GREEN),
    ("2 · CONTEXT DELTA", '"Is that evidence still valid?"',
     "UNCHANGED · CHANGED · REMOVED · UNVERIFIABLE.", "✓  BUILT", GREEN),
    ("3 · CROSS-AGENT HANDOFF", '"What should Agent B use now?"',
     "Delta + fresh ctx + new proof. Tiny reference.", "◆  HACKATHON BUILD", AMBER),
]
for i, (h, q, d, tag, c) in enumerate(blocks):
    x = 0.7 + i * 4.0
    shp = box(s, x, 2.25, 3.7, 1.9, fill=ACCENT_DIM if i != 1 else PANEL, border=c)
    set_text(shp, [(h, 15, c, True, PP_ALIGN.LEFT), (q, 13.5, TEXT, False, PP_ALIGN.LEFT),
                   (d, 12, MUTED, False, PP_ALIGN.LEFT), (tag, 11.5, c, True, PP_ALIGN.LEFT)], anchor=MSO_ANCHOR.MIDDLE)
# flow strip
flow = ["Agent A", "Context", "PROOF A", "repo changes", "DELTA", "Agent B", "fresh ctx", "PROOF B"]
fx, fw, gap = 0.7, 1.28, 0.22
for i, f in enumerate(flow):
    x = fx + i * (fw + gap)
    hot = f in ("PROOF A", "DELTA", "PROOF B")
    shp = box(s, x, 4.5, fw, 0.62, fill=ACCENT_DIM if hot else PANEL, border=ACCENT if hot else BORDER)
    set_text(shp, [(f, 12, TEXT if hot else MUTED, True, PP_ALIGN.CENTER)])
    if i < len(flow) - 1:
        arrow(s, x + fw, 4.81, x + fw + gap, 4.81, width=1.5)
cb = box(s, 0.7, 5.45, 6.1, 1.25, fill=PANEL2)
set_text(cb, [("No chat-history replay required.", 17, TEXT, True, PP_ALIGN.CENTER)])
cb2 = box(s, 7.0, 5.45, 5.6, 1.25, fill=PANEL2)
set_text(cb2, [("Handoff reference is tiny; intelligence stays local.", 15, MUTED, False, PP_ALIGN.CENTER)])
footer(s)
s.notes_slide.placeholders[1].text_frame.text = (
    "The differentiator is continuity: Proof records exactly what Agent A saw. "
    "Delta checks if it is still valid — unchanged, changed, removed, or unverifiable rather than guessed. "
    "Handoff gives Agent B the delta plus fresh context and a new proof. No chat replay needed. "
    "Proof and Delta are already built — Handoff is what we complete at iQOO."
)

# ============ SLIDE 5 — DEMO ============
s = new_slide()
slide_head(s, "LIVE DEMO", "What the demo looks like  —  retry.rs · unrelated.rs")
steps = [
    ("STEP 1 · Agent A asks", '"Where is retry policy enforced?"', "Returns evidence + Proof A.", ACCENT),
    ("STEP 2 · Dev edits unrelated.rs", "Repository changes elsewhere.", "Retry evidence unaffected.", MUTED),
    ("STEP 3 · Agent B starts fresh", "Only gets: query + Proof A.", "No chat history transferred.", ACCENT),
    ("STEP 4 · Engine answers", "UNCHANGED → fresh ctx → Proof B.", "Continuity without replay.", GREEN),
]
for i, (h, q, d, c) in enumerate(steps):
    x = 0.7 + i * 3.0
    shp = box(s, x, 2.25, 2.85, 2.3, fill=ACCENT_DIM if i in (0, 3) else PANEL, border=c if i in (0, 3) else BORDER)
    set_text(shp, [(h, 13.5, c, True, PP_ALIGN.LEFT), (q, 13, TEXT, False, PP_ALIGN.LEFT),
                   (d, 12, MUTED, False, PP_ALIGN.LEFT)], anchor=MSO_ANCHOR.MIDDLE)
    if i < 3:
        arrow(s, x + 2.85, 3.4, x + 3.0, 3.4, width=1.5)
# second example strip
sec = box(s, 0.7, 4.85, 11.9, 1.85, fill=PANEL2)
set_text(sec, [("THEN: dev modifies retry.rs  →  Agent C continues from Proof B  →  Delta reports CHANGED  →  fresh implementation context returned.",
                15, TEXT, False, PP_ALIGN.CENTER),
               ("Same workflow proves both paths: untouched evidence stays valid, edited evidence is caught.", 13, MUTED, False, PP_ALIGN.CENTER)],
         anchor=MSO_ANCHOR.MIDDLE)
footer(s)
s.notes_slide.placeholders[1].text_frame.text = (
    "Demo in 30 seconds: Agent A asks about retry policy and gets Proof A. "
    "An unrelated edit leaves retry evidence UNCHANGED — Agent B continues with fresh context and Proof B. "
    "Then a real edit to retry.rs shows CHANGED with fresh implementation context for Agent C."
)

# ============ SLIDE 6 — CREDIBILITY ============
s = new_slide()
slide_head(s, "WHY IT MATTERS", "Why this matters when developers use multiple AI agents.")
outcomes = [
    ("LESS REDISCOVERY", "One shared repository intelligence layer instead of repeated exploration."),
    ("SAFER CONTINUITY", "Old evidence is checked before reuse — never trusted blindly."),
    ("AGENT INDEPENDENCE", "Switch agents without transferring entire chats."),
]
for i, (h, d) in enumerate(outcomes):
    x = 0.7 + i * 4.0
    shp = accent_box(s, x, 2.35, 3.7, 1.75) if i == 1 else box(s, x, 2.35, 3.7, 1.75)
    set_text(shp, [(h, 17, ACCENT, True, PP_ALIGN.LEFT), (d, 13.5, TEXT, False, PP_ALIGN.LEFT)], anchor=MSO_ANCHOR.MIDDLE)
pills = [("Local-first", "intelligence stays on machine"),
         ("Deterministic", "Proof → Delta → Handoff"),
         ("Shared runtime", "one resource-managed daemon"),
         ("Conservative", "UNVERIFIABLE > guessing")]
for i, (h, d) in enumerate(pills):
    x = 0.7 + i * 3.0
    shp = box(s, x, 4.55, 2.85, 1.15)
    set_text(shp, [(h, 14, ACCENT, True, PP_ALIGN.CENTER), (d, 11.5, MUTED, False, PP_ALIGN.CENTER)])
label(s, 0.7, 6.0, 11.9, 0.5, "Search finds code. Context Engine makes repo context survive safely across agent sessions.",
      14, MUTED, align=PP_ALIGN.CENTER)
footer(s)
s.notes_slide.placeholders[1].text_frame.text = (
    "This is infrastructure, not another agent: less rediscovery through one shared index, "
    "safer continuity through verified evidence, and agent independence — switch agents without moving chats. "
    "Local-first, deterministic Proof-to-Delta-to-Handoff, one shared daemon, conservative by design."
)

# ============ SLIDE 7 — PLAN + TEAM ============
s = new_slide()
slide_head(s, "HACKATHON PLAN", "What we will deliver at iQOO Hackathon 2026")
# left: already built
lb = box(s, 0.7, 2.25, 3.7, 3.0)
set_text(lb, [("ALREADY BUILT", 14, GREEN, True, PP_ALIGN.LEFT)], anchor=MSO_ANCHOR.TOP)
built = ["structural repo indexing", "hybrid retrieval (exact/BM25/sem)", "shared daemon + MCP interface", "Context Proof (merged)", "Context Delta (merged)"]
for i, item in enumerate(built):
    label(s, 1.0, 2.85 + i * 0.42, 3.2, 0.4, "✓  " + item, 12.5, TEXT)
# right: hackathon build
rb = accent_box(s, 4.7, 2.25, 4.1, 3.0)
set_text(rb, [("HACKATHON BUILD", 14, ACCENT, True, PP_ALIGN.LEFT)], anchor=MSO_ANCHOR.TOP)
build = ["Cross-Agent Handoff integration", "independent Agent A → B workflow", "live MCP demo + dev workflow", "controlled A/B evaluation:"]
for i, item in enumerate(build):
    label(s, 5.0, 2.85 + i * 0.38, 3.6, 0.4, "→  " + item, 12.5, TEXT)
for i, item in enumerate(["task completion · repo exploration", "tool calls · context usage"]):
    label(s, 5.35, 4.37 + i * 0.36, 3.3, 0.4, item, 12, MUTED)
# team
team = box(s, 9.1, 2.25, 3.5, 3.0, fill=PANEL2)
set_text(team, [("TEAM MULANOUS", 14, TEXT, True, PP_ALIGN.LEFT),
                ("Sandeep Gurram", 15, ACCENT, True, PP_ALIGN.LEFT),
                ("mobile/web systems · backend architecture · Rust/Go/TS · GCP/Postgres", 11.5, MUTED, False, PP_ALIGN.LEFT),
                ("retrieval infra · AI-agent workflows · Google hackathon participant", 11.5, MUTED, False, PP_ALIGN.LEFT)],
         anchor=MSO_ANCHOR.TOP)
# disclosure
disc = box(s, 0.7, 5.4, 11.9, 1.0, fill=PANEL, border=AMBER)
set_text(disc, [("HACKATHON SCOPE", 13, AMBER, True, PP_ALIGN.CENTER),
                ("Pre-existing:  indexing · hybrid retrieval · shared daemon · Context Proof · Context Delta", 12.5, TEXT, False, PP_ALIGN.CENTER),
                ("Built at iQOO:  Cross-Agent Handoff · demo integration · A/B evaluation", 12.5, TEXT, False, PP_ALIGN.CENTER)],
         anchor=MSO_ANCHOR.MIDDLE)
label(s, 0.7, 6.5, 11.9, 0.4, "One codebase. Many agents. Continuous context.", 19, TEXT, True, PP_ALIGN.CENTER)
footer(s)
s.notes_slide.placeholders[1].text_frame.text = (
    "Proof and Delta already work. At iQOO we turn those foundations into the cross-agent handoff workflow "
    "and measure it in a controlled A/B demo: task completion, repo exploration, tool calls, context usage. "
    "The result we want: Agent B solves the task without rediscovering the repository. "
    "Pre-existing work is disclosed on the slide. One codebase, many agents, continuous context."
)

prs.save(PPTX_PATH)
print("saved", PPTX_PATH, "slides:", len(prs.slides._sldIdLst))
