/**
 * The fields the mutators create, when blockly-ext's field types are registered: they come from
 * Blockly's field registry with the blockly-ext configuration, and a parameter's name field holds
 * the declaration `{sym, name}`. Stand-in field classes play the registered types here.
 */
import * as Blockly from 'blockly/core';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import {
  createJoinField,
  createModeField,
  createNameField,
  createTypeField,
  readName,
} from './fields';
import { isB2cMutatorBlock, registerB2cMutators } from './register';
import { defineTestBlocks } from './test-fixtures';
import { B2C_MUTATOR_ITEMS, type ParamRow } from './types';

const configs = new Map<string, Record<string, unknown>>();

function pairs(value: unknown): [string, string][] {
  return Array.isArray(value) ? (value as [string, string][]) : [];
}

class TestTypeField extends Blockly.FieldDropdown {
  static override fromJson(options: Blockly.FieldConfig & { types?: unknown }): TestTypeField {
    configs.set('b2c_type', { ...options });
    const types = Array.isArray(options.types) ? (options.types as string[]) : [];
    return new TestTypeField(types.map((type) => [type, type]));
  }
}

class TestDropdownField extends Blockly.FieldDropdown {
  static override fromJson(
    options: Blockly.FieldConfig & { options?: unknown },
  ): TestDropdownField {
    configs.set('b2c_dropdown', { ...options });
    return new TestDropdownField(pairs(options.options));
  }
}

class TestDeclField extends Blockly.Field<unknown> {
  static override fromJson(options: Blockly.FieldConfig & { value?: unknown }): TestDeclField {
    configs.set('b2c_symbol_decl', { ...options });
    return new TestDeclField(options.value ?? null);
  }

  protected override doClassValidation_(value?: unknown): unknown {
    return typeof value === 'object' && value !== null && 'sym' in value && 'name' in value
      ? value
      : null;
  }
}

const REGISTERED = ['b2c_type', 'b2c_dropdown', 'b2c_symbol_decl'];

let workspace: Blockly.Workspace;

beforeAll(() => {
  Blockly.fieldRegistry.register('b2c_type', TestTypeField);
  Blockly.fieldRegistry.register('b2c_dropdown', TestDropdownField);
  Blockly.fieldRegistry.register('b2c_symbol_decl', TestDeclField);
  registerB2cMutators();
  defineTestBlocks();
});

afterAll(() => {
  for (const type of REGISTERED) {
    Blockly.fieldRegistry.unregister(type);
  }
});

beforeEach(() => {
  configs.clear();
  workspace = new Blockly.Workspace();
});

afterEach(() => {
  workspace.dispose();
});

describe('fields from the registry', () => {
  it('are created with the blockly-ext configuration', () => {
    expect(createTypeField(['int', 'std::string'], 'std::string')).toBeInstanceOf(TestTypeField);
    expect(configs.get('b2c_type')).toMatchObject({
      type: 'b2c_type',
      types: ['int', 'std::string'],
      value: 'std::string',
      ariaLabel: 'Parameter type',
    });
    const mode = createModeField('editable');
    expect(mode).toBeInstanceOf(TestDropdownField);
    expect(mode.getValue()).toBe('editable');
    expect(configs.get('b2c_dropdown')).toMatchObject({
      options: [
        ['copy', 'copy'],
        ['editable', 'editable'],
        ['read-only', 'read_only'],
      ],
      value: 'editable',
      ariaLabel: 'Parameter mode',
    });
    const name = createNameField('sym_a', 'apples');
    expect(name.holdsDecl).toBe(true);
    expect(name.field.getValue()).toEqual({ sym: 'sym_a', name: 'apples' });
    expect(readName(name)).toBe('apples');
    expect(configs.get('b2c_symbol_decl')).toMatchObject({
      value: { sym: 'sym_a', name: 'apples' },
    });
  });

  it('give way to the built-in fields when a field class refuses its configuration', () => {
    class Refusing extends Blockly.FieldDropdown {
      static override fromJson(): Refusing {
        throw new TypeError('bad config');
      }
    }
    Blockly.fieldRegistry.unregister('b2c_dropdown');
    Blockly.fieldRegistry.register('b2c_dropdown', Refusing);
    try {
      const field = createJoinField({
        name: 'OP',
        kind: 'dropdown',
        options: [
          ['and', 'and'],
          ['or', 'or'],
        ],
        types: [],
        default: 'or',
      });
      expect(field).toBeInstanceOf(Blockly.FieldDropdown);
      expect(field).not.toBeInstanceOf(Refusing);
      expect(field.getValue()).toBe('or');
    } finally {
      Blockly.fieldRegistry.unregister('b2c_dropdown');
      Blockly.fieldRegistry.register('b2c_dropdown', TestDropdownField);
    }
  });

  it('read a name from a declaration value or plain text, and nothing else', () => {
    const name = createNameField('sym_a', 'a');
    expect(readName({ field: name.field, holdsDecl: true })).toBe('a');
    expect(readName({ field: new Blockly.FieldTextInput('x'), holdsDecl: false })).toBe('x');
    const odd = new TestDeclField({ sym: 'sym_a', name: 3 });
    expect(readName({ field: odd, holdsDecl: true })).toBeNull();
    expect(readName({ field: new Blockly.FieldNumber(5), holdsDecl: false })).toBeNull();
  });

  it('use the declaration API of a field whose value is the name', () => {
    /** Like blockly-ext's b2c_symbol_decl: the value is the name, the ID is kept beside it. */
    class NameValueDeclField extends Blockly.FieldTextInput {
      private sym: string | null = null;

      getDecl(): { sym: string; name: string } | null {
        const name = this.getValue();
        return this.sym === null || name === null ? null : { sym: this.sym, name };
      }

      setDecl(value: { sym: string; name: string }): boolean {
        this.sym = value.sym;
        this.setValue(value.name);
        return true;
      }

      static override fromJson(): NameValueDeclField {
        return new NameValueDeclField('');
      }
    }
    Blockly.fieldRegistry.unregister('b2c_symbol_decl');
    Blockly.fieldRegistry.register('b2c_symbol_decl', NameValueDeclField);
    try {
      const name = createNameField('sym_b', 'bananas');
      expect(name.field).toBeInstanceOf(NameValueDeclField);
      expect(name.field.getValue()).toBe('bananas');
      expect((name.field as NameValueDeclField).getDecl()).toEqual({
        sym: 'sym_b',
        name: 'bananas',
      });
      name.field.setValue('pears');
      expect(readName(name)).toBe('pears');

      const block = workspace.newBlock('func.define');
      if (!isB2cMutatorBlock(block)) {
        throw new Error('no mutator');
      }
      block.b2cSetExtra({ params: [{ sym: 'sym_n', name: 'n', type: 'int', mode: 'copy' }] });
      block.setFieldValue('count', 'B2C_PARAM0_NAME');
      expect(block.b2cGetExtra()).toEqual({
        params: [{ sym: 'sym_n', name: 'count', type: 'int', mode: 'copy' }],
      });
    } finally {
      Blockly.fieldRegistry.unregister('b2c_symbol_decl');
      Blockly.fieldRegistry.register('b2c_symbol_decl', TestDeclField);
    }
  });
});

describe('parameter rows with the registered fields', () => {
  it('keep each row’s symbol in its declaration field', () => {
    const block = workspace.newBlock('func.define');
    if (!isB2cMutatorBlock(block)) {
      throw new Error('no mutator');
    }
    const params: ParamRow[] = [{ sym: 'sym_n', name: 'n', type: 'int', mode: 'copy' }];
    block.b2cSetExtra({ params });
    expect(block.getField('B2C_PARAM0_NAME')).toBeInstanceOf(TestDeclField);
    expect(block.getFieldValue('B2C_PARAM0_NAME')).toEqual({ sym: 'sym_n', name: 'n' });
    block.setFieldValue({ sym: 'sym_n', name: 'count' }, 'B2C_PARAM0_NAME');
    expect(block.b2cGetExtra()).toEqual({ params: [{ ...params[0], name: 'count' }] });
  });
});

describe('logic.operation drawn without its operator', () => {
  it('gets a registered dropdown from the mutator', () => {
    Blockly.Blocks['logic.operation'] = {
      init(this: Blockly.Block): void {
        this.setOutput(true);
        Blockly.Extensions.apply(B2C_MUTATOR_ITEMS, this, true);
      },
    };
    const block = workspace.newBlock('logic.operation');
    expect(block.getField('OP')).toBeInstanceOf(TestDropdownField);
    expect(block.getFieldValue('OP')).toBe('and');
    expect(configs.get('b2c_dropdown')).toMatchObject({ ariaLabel: 'Operator', value: 'and' });
  });
});
