"""Adds timing probes to a (stock or patched) Blockly 12.5.1 bundle, for a diagnostic build only.

Usage: python3 -I instrument_bundle.py INPUT OUTPUT

Records into globalThis.__p3 (milliseconds and counts): each render pass's phases (drawing,
resizeContents, component locations, bumpNeighbours, dequeue) and the number of blocks drawn; and
a gesture's setTargetBlock split into focusNode and bringToFront, with the SVG moves bringToFront
makes.
"""
import sys

RM = "$$module$build$src$core$render_management"
GROUP_GET = "$.getGroup$$module$build$src$core$events$utils"
GROUP_SET = "$.setGroup$$module$build$src$core$events$utils"
FOCUS = "$.getFocusManager$$module$build$src$core$focus_manager"

PROBE = (
    "(globalThis.__p3||(globalThis.__p3={passes:0,roots:0,drawn:0,render:0,resize:0,locate:0,"
    "bump:0,dequeue:0,focus:0,front:0,moves:0,gestures:0}))"
)


def doRenders(locate: str) -> str:
    return (
        f"doRenders{RM}=function(a){{const P={PROBE},T=()=>performance.now();let t=T(),u;"
        f"const b=a?new Set([a]):new Set([...rootBlocks{RM}].map(f=>f.workspace)),"
        f"c=[...rootBlocks{RM}].filter(shouldRenderRootBlock{RM}).filter(f=>b.has(f.workspace));"
        f"P.passes++;P.roots+=c.length;"
        f"for(var d of c)renderBlock{RM}(d);u=T();P.render+=u-t;t=u;"
        f"for(const f of b)f.resizeContents();u=T();P.resize+=u-t;t=u;"
        f"for(var e of c)d=e.getRelativeToSurfaceXY(),{locate};u=T();P.locate+=u-t;t=u;"
        f"for(const f of c)e={GROUP_GET}(),(d=eventGroups{RM}.get(f))&&{GROUP_SET}(d),"
        f"f.bumpNeighbours(),{GROUP_SET}(e);u=T();P.bump+=u-t;t=u;"
        f"for(const f of c)dequeueBlock{RM}(f);u=T();P.dequeue+=u-t;"
        f"a||(afterRendersPromise{RM}=null)}};"
    )


def main() -> None:
    source, output = sys.argv[1], sys.argv[2]
    text = open(source, encoding="utf-8").read()
    start = text.index(f"doRenders{RM}=function(a){{")
    end = text.index(f"dequeueBlock{RM}=function(a){{", start)
    old = text[start:end]
    patched = f"updateComponentLocations{RM}(e,d)" in old
    locate = f"updateComponentLocations{RM}(e,d)" if patched else "e.updateComponentLocations(d)"
    text = text[:start] + doRenders(locate) + "\n" + text[end:]
    replacements = [
        (
            "a.renderEfficiently()}};",
            f"(a.renderEfficiently(),{PROBE}.drawn++)}}}};",
        ),
        (
            f"setTargetBlock(a){{a.isShadow()?this.setTargetBlock(a.getParent()):(this.targetBlock=a,"
            f"{FOCUS}().focusNode(this.targetBlock),this.targetBlock.bringToFront())}}",
            f"setTargetBlock(a){{if(a.isShadow())this.setTargetBlock(a.getParent());else{{"
            f"const P={PROBE};let t=performance.now();this.targetBlock=a;"
            f"{FOCUS}().focusNode(this.targetBlock);let u=performance.now();P.focus+=u-t;t=u;"
            f"this.targetBlock.bringToFront();P.front+=performance.now()-t;P.gestures++}}}}",
        ),
        (
            "f[f.length-1]!==d&&e.appendChild(d);if(a)break;",
            f"f[f.length-1]!==d&&(e.appendChild(d),{PROBE}.moves++);if(a)break;",
        ),
    ]
    for old_text, new_text in replacements:
        count = text.count(old_text)
        if count != 1:
            sys.exit(f"expected one occurrence, found {count}: {old_text[:80]}")
        text = text.replace(old_text, new_text)
    open(output, "w", encoding="utf-8").write(text)


if __name__ == "__main__":
    main()
