"""Applies the P-3 render-management spike to Blockly 12.5.1's blockly_compressed.js.

Usage: python3 -I patch_bundle.py ORIGINAL OUTPUT

The same change as upstream/core/{render_management,block}.ts, written in the bundle's compiled
form. Every replaced text must occur exactly once.
"""
import hashlib
import sys

ORIGINAL_SHA256 = "2496815c27b415707c98c808489cdeddf27b45431af35d275f67e8c2c36c903a"
RM = "$$module$build$src$core$render_management"
COORD = "Coordinate$$module$build$src$core$utils$coordinate"
GROUP = "$.getGroup$$module$build$src$core$events$utils"

REPLACEMENTS = [
    # The new set, and the location walk, declared with the module's other state.
    (
        f"dirtyBlocks{RM}=new WeakSet,",
        f"dirtyBlocks{RM}=new WeakSet,blocksToRender{RM}=new WeakSet,"
        f"updateComponentLocations{RM}=function(a,b){{if(blocksToRender{RM}.has(a))"
        f"a.updateComponentLocations(b);else for(const c of a.getChildren(!1))"
        f"dirtyBlocks{RM}.has(c)&&updateComponentLocations{RM}(c,{COORD}.sum(b,c.relativeCoords))}},",
    ),
    # queueBlock: walk up iteratively; draw the block, its parent, and each block whose input
    # (not next connection) leads to the change.
    (
        f"queueBlock{RM}=function(a){{dirtyBlocks{RM}.add(a);eventGroups{RM}.set(a,{GROUP}());"
        f"const b=a.getParent();b?queueBlock{RM}(b):rootBlocks{RM}.add(a)}};",
        f"queueBlock{RM}=function(a){{const b={GROUP}();let c=null,d=a;for(;d;){{"
        f"dirtyBlocks{RM}.add(d);eventGroups{RM}.set(d,b);"
        f"c&&c!==a&&d.getNextBlock()===c&&!d.outputConnection||blocksToRender{RM}.add(d);"
        f"const e=d.getParent();e||rootBlocks{RM}.add(d);c=d;d=e}}}};",
    ),
    # doRenders: update the locations of what may have moved only.
    (
        "for(var e of c)d=e.getRelativeToSurfaceXY(),e.updateComponentLocations(d);",
        f"for(var e of c)d=e.getRelativeToSurfaceXY(),updateComponentLocations{RM}(e,d);",
    ),
    # dequeueBlock: forget the new mark too.
    (
        f"dirtyBlocks{RM}.delete(a);eventGroups{RM}.delete(a);",
        f"dirtyBlocks{RM}.delete(a);blocksToRender{RM}.delete(a);eventGroups{RM}.delete(a);",
    ),
    # renderBlock: draw only the blocks whose shape may have changed.
    (
        f"for(const b of a.getChildren(!1))renderBlock{RM}(b);a.renderEfficiently()}}}};",
        f"for(const b of a.getChildren(!1))renderBlock{RM}(b);"
        f"blocksToRender{RM}.has(a)&&a.renderEfficiently()}}}};",
    ),
    # Block.getDescendants: an explicit stack, in the same (pre-)order.
    (
        "getDescendants(a){const b=[this],c=this.getChildren(a);for(let d,e=0;d=c[e];e++)"
        "b.push(...d.getDescendants(a));return b}",
        "getDescendants(a){const b=[],c=[this];for(let d;d=c.pop();){b.push(d);"
        "const e=d.getChildren(a);for(let f=e.length-1;0<=f;f--)c.push(e[f])}return b}",
    ),
]


def main() -> None:
    original, output = sys.argv[1], sys.argv[2]
    data = open(original, "rb").read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != ORIGINAL_SHA256:
        sys.exit(f"{original} is not Blockly 12.5.1's bundle (sha256 {digest})")
    text = data.decode("utf-8")
    for old, new in REPLACEMENTS:
        count = text.count(old)
        if count != 1:
            sys.exit(f"expected one occurrence, found {count}: {old[:80]}")
        text = text.replace(old, new)
    with open(output, "w", encoding="utf-8") as handle:
        handle.write(text)


if __name__ == "__main__":
    main()
