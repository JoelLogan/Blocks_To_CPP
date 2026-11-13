/**
 * The *namespace* block's mouth (docs/plans/m3-contracts/w3-modules.md §2.12, §5.2 N1): the
 * definitions that are namespace members keep the hat look at the top level and show notches in a
 * mouth (the D2 spike's result, §7.1); a mouth takes definitions and refuses statements; nothing
 * stacks under a top-level definition; a definition snaps in and out; a drag deeper than 14
 * namespaces is refused, while a file that holds one still loads. And *use namespace*'s field.
 */
import * as Blockly from 'blockly/core';
import { beforeEach, describe, expect, it } from 'vitest';

import {
  b2cLightTheme,
  B2C_CHECKER_NAME,
  B2cNamespaceField,
  B2cSymbolDeclField,
  isInMouth,
  MAX_NAMESPACE_DEPTH,
  namespaceDepthAt,
  namespaceHeight,
  readB2cField,
  STD_NAMESPACE_VALUE,
  writeB2cField,
} from '../src';
import { fakeSymbols, setUpBlocks } from './helpers';

const FAR = 1e9;
const MEMBERS = ['func.define', 'type.struct', 'type.enum', 'var.global', 'program.namespace'];

let workspace: Blockly.WorkspaceSvg;

beforeEach(() => {
  setUpBlocks();
  const host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, {
    renderer: 'zelos',
    theme: b2cLightTheme,
    sounds: false,
    plugins: { connectionChecker: B2C_CHECKER_NAME },
  });
  return () => {
    workspace.dispose();
    document.body.replaceChildren();
  };
});

function rendered(type: string): Blockly.BlockSvg {
  const block = workspace.newBlock(type);
  block.initSvg();
  block.render();
  return block;
}

function mouth(block: Blockly.Block): Blockly.Connection {
  const connection = block.getInput('DEFINITIONS')?.connection;
  if (connection === null || connection === undefined) {
    throw new Error(`${block.type} has no mouth`);
  }
  return connection;
}

function previous(block: Blockly.Block): Blockly.Connection {
  if (block.previousConnection === null) {
    throw new Error(`${block.type} has no previous connection`);
  }
  return block.previousConnection;
}

function next(block: Blockly.Block): Blockly.Connection {
  if (block.nextConnection === null) {
    throw new Error(`${block.type} has no next connection`);
  }
  return block.nextConnection;
}

function outline(block: Blockly.BlockSvg): string | null {
  return block.pathObject.svgPath.getAttribute('d');
}

/** Whether the checker lets `child` go to `parent`, as a drop (dragging) or as a load. */
function fits(parent: Blockly.Connection, child: Blockly.Connection, dragging = false): boolean {
  return workspace.connectionChecker.canConnect(child, parent, dragging, FAR);
}

describe('the look of a namespace member (the D2 spike)', () => {
  it.each(MEMBERS)('%s keeps the hat at the top level and shows notches in a mouth', (type) => {
    const block = rendered(type);
    const top = outline(block);
    // The outline the block had before namespaces: a definition without connections.
    const plain = rendered(type);
    plain.setPreviousStatement(false);
    plain.setNextStatement(false);
    plain.render();
    expect(top).toBe(outline(plain));

    const namespace = rendered('program.namespace');
    mouth(namespace).connect(previous(block));
    block.render();
    const inside = outline(block);
    expect(inside).not.toBe(top);
    expect(isInMouth(block)).toBe(true);

    block.unplug();
    block.render();
    expect(outline(block)).toBe(top);
    expect(isInMouth(block)).toBe(false);
  });
});

describe('the mouth', () => {
  it('takes every namespace member, one below another', () => {
    const namespace = rendered('program.namespace');
    let place = mouth(namespace);
    for (const type of MEMBERS) {
      const block = rendered(type);
      expect(fits(place, previous(block)), type).toBe(true);
      expect(fits(place, previous(block), true), type).toBe(true);
      place.connect(previous(block));
      place = next(block);
    }
    expect(namespace.getDescendants(false)).toHaveLength(MEMBERS.length + 1);
  });

  it('refuses statements, the program, use header and use namespace', () => {
    const namespace = rendered('program.namespace');
    const member = rendered('func.define');
    mouth(namespace).connect(previous(member));
    for (const type of ['io.print', 'var.declare', 'control.forever']) {
      const statement = rendered(type);
      expect(fits(mouth(namespace), previous(statement)), type).toBe(false);
      expect(fits(next(member), previous(statement)), type).toBe(false);
    }
    // The other definitions have no previous connection to offer.
    for (const type of ['program.main', 'program.use_header', 'program.use_namespace']) {
      expect(rendered(type).previousConnection, type).toBeNull();
    }
  });

  it('never lets a definition stack under a top-level one or go into a statement list', () => {
    const top = rendered('func.define');
    const other = rendered('type.enum');
    expect(fits(next(top), previous(other))).toBe(false);
    expect(fits(next(top), previous(other), true)).toBe(false);
    // Nor a statement under a top-level definition.
    expect(fits(next(top), previous(rendered('io.print')))).toBe(false);
    const loop = rendered('control.forever');
    const body = loop.getInput('BODY')?.connection;
    expect(body).toBeDefined();
    if (body !== null && body !== undefined) {
      expect(fits(body, previous(other))).toBe(false);
    }
    expect(fits(next(rendered('io.print')), previous(other))).toBe(false);
  });

  it('lets a definition snap in and out', () => {
    const namespace = rendered('program.namespace');
    const first = rendered('func.define');
    const second = rendered('type.struct');
    mouth(namespace).connect(previous(first));
    next(first).connect(previous(second));
    expect(isInMouth(second)).toBe(true);
    second.unplug();
    expect(isInMouth(second)).toBe(false);
    expect(namespace.getDescendants(false)).toHaveLength(2);
    expect(fits(next(first), previous(second), true)).toBe(true);
  });
});

describe('nesting', () => {
  /** Namespaces nested `depth` deep; returns the outermost and the innermost. */
  function nested(depth: number): { outer: Blockly.BlockSvg; inner: Blockly.BlockSvg } {
    const outer = rendered('program.namespace');
    let inner = outer;
    for (let level = 2; level <= depth; level++) {
      const block = rendered('program.namespace');
      mouth(inner).connect(previous(block));
      inner = block;
    }
    return { outer, inner };
  }

  it('counts the namespaces around a place and those a block brings', () => {
    const { outer, inner } = nested(3);
    expect(namespaceDepthAt(mouth(inner))).toBe(3);
    expect(namespaceHeight(outer)).toBe(3);
    const member = rendered('func.define');
    mouth(inner).connect(previous(member));
    expect(namespaceDepthAt(next(member))).toBe(3);
    expect(namespaceHeight(member)).toBe(0);
  });

  it(`refuses a drop deeper than ${String(MAX_NAMESPACE_DEPTH)}, but not a load`, () => {
    const { inner } = nested(MAX_NAMESPACE_DEPTH - 1);
    // A 14th level fits.
    const fourteenth = rendered('program.namespace');
    expect(fits(mouth(inner), previous(fourteenth), true)).toBe(true);
    // Two more levels at once do not.
    const two = nested(2).outer;
    expect(fits(mouth(inner), previous(two), true)).toBe(false);
    mouth(inner).connect(previous(fourteenth));
    const fifteenth = rendered('program.namespace');
    expect(fits(mouth(fourteenth), previous(fifteenth), true)).toBe(false);
    // A definition that brings no namespace still fits at 14.
    expect(fits(mouth(fourteenth), previous(rendered('func.define')), true)).toBe(true);
    // Loading a hand-edited file connects it; the analyser reports it (B2C-E0219).
    expect(fits(mouth(fourteenth), previous(fifteenth))).toBe(true);
  });
});

describe('use namespace', () => {
  function named(block: Blockly.Block, sym: string, name: string): void {
    const field = block.getField('NAME');
    expect(field).toBeInstanceOf(B2cSymbolDeclField);
    if (field !== null) {
      expect(writeB2cField(field, { sym, name })).toBe(true);
    }
  }

  it('offers std and the top-level namespaces of the canvas, each name once', () => {
    const geo = rendered('program.namespace');
    named(geo, 'sym_b', 'geo');
    const geoAgain = rendered('program.namespace');
    named(geoAgain, 'sym_a', 'geo');
    const shapes = rendered('program.namespace');
    named(shapes, 'sym_c', 'shapes');
    // A nested namespace is not offered (06 §6.14.3 rule 6).
    const inner = rendered('program.namespace');
    named(inner, 'sym_d', 'inner');
    mouth(shapes).connect(previous(inner));

    const use = rendered('program.use_namespace');
    const field = use.getField('NS');
    expect(field).toBeInstanceOf(B2cNamespaceField);
    if (!(field instanceof B2cNamespaceField)) {
      return;
    }
    expect(readB2cField(field)).toBe(STD_NAMESPACE_VALUE);
    expect(field.getText()).toBe('std');
    const [geoBlock] = [geo, geoAgain].sort((a, b) => (a.id < b.id ? -1 : 1));
    const geoSym = geoBlock === geo ? 'sym_b' : 'sym_a';
    expect(field.getOptions(false)).toEqual([
      ['std', '::std'],
      ['geo', geoSym],
      ['shapes', 'sym_c'],
    ]);
    expect(writeB2cField(field, 'sym_c')).toBe(true);
    expect(field.getText()).toBe('shapes');
  });

  it('keeps a value the menu does not list and shows it by name when known', () => {
    fakeSymbols({}, { sym_far: 'far' });
    const use = rendered('program.use_namespace');
    const field = use.getField('NS');
    if (!(field instanceof B2cNamespaceField)) {
      throw new Error('NS is not a namespace field');
    }
    expect(writeB2cField(field, 'sym_far')).toBe(true);
    expect(field.getText()).toBe('far');
    expect(field.getOptions(false)).toContainEqual(['far', 'sym_far']);
    expect(writeB2cField(field, '::std::chrono')).toBe(true);
    expect(field.getText()).toBe('std::chrono');
    expect(writeB2cField(field, 'sym_gone')).toBe(true);
    expect(field.getText()).toBe('sym_gone');
    expect(readB2cField(field)).toBe('sym_gone');
  });
});
