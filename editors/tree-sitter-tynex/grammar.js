// Editor grammar only. Runtime validity and typing belong to the Rust compiler.
// Extend a pinned, unmodified upstream Lua grammar (see vendor/lua/README.md).
import lua from './vendor/lua/grammar.js';
const list = rule => seq(rule, repeat(seq(',', rule)));
const binding = $ => seq(field('name', $.identifier), optional($.type_annotation));
export default grammar(lua, {
  name: 'tynex',
  conflicts: $ => [[$.return_type, $.parenthesized_type]],
  // Keep a malformed member from consuming the next end/trait/class as a bare field.
  reserved: {
    global: _ => ['and', 'break', 'do', 'else', 'elseif', 'end', 'false', 'for',
      'function', 'goto', 'if', 'in', 'local', 'nil', 'not', 'or', 'repeat',
      'return', 'then', 'true', 'until', 'while', 'class', 'trait', 'import',
      'from', 'declare', 'try', 'except'],
  },
  rules: {
    identifier: _ => /[A-Za-z_][A-Za-z0-9_]*/,
    statement: ($, original) => choice(original, $.import_statement, $.try_statement, $.optional_function_call),
    expression: ($, original) => choice(original,
      $.optional_dot_index_expression, $.optional_bracket_index_expression, $.optional_function_call),
    _prefix_expression: ($, original) => prec(1, choice(original,
      $.optional_dot_index_expression, $.optional_bracket_index_expression, $.optional_function_call)),
    optional_dot_index_expression: $ => seq(
      field('table', $._prefix_expression), '?.', field('field', $.identifier)
    ),
    optional_bracket_index_expression: $ => seq(
      field('table', $._prefix_expression), '?[', field('field', $.expression), ']'
    ),
    optional_function_call: $ => choice(
      seq(field('name', $._prefix_expression), field('arguments', $.optional_arguments)),
      seq(field('name', $.optional_method_index_expression), field('arguments', $.arguments))
    ),
    optional_arguments: $ => seq('?(', optional(list($.expression)), ')'),
    optional_method_index_expression: $ => seq(
      field('table', $._prefix_expression), '?:', field('method', $.identifier)
    ),
    declaration: ($, original) => choice(original, $.export_declaration, $.declare_statement, $.class_declaration, $.trait_declaration),
    class_declaration: $ => seq(
      'class', field('name', $.identifier), optional($.trait_list),
      repeat(choice($.class_field, $.method_declaration, $.empty_statement)), 'end'
    ),
    trait_declaration: $ => seq(
      'trait', field('name', $.identifier),
      repeat(choice($.trait_requirement, $.method_declaration, $.empty_statement)), 'end'
    ),
    trait_list: $ => seq('is', list($.named_type)),
    class_field: $ => prec.right(seq(
      field('name', $.identifier),
      optional($.type_annotation), optional(seq('=', field('value', $.expression))), optional(';')
    )),
    trait_requirement: $ => prec.right(seq(field('name', $.identifier), $.type_annotation, optional(';'))),
    method_declaration: $ => seq('function', field('name', $.identifier), $._function_body),
    declare_statement: $ => prec.right(seq('declare', field('name', $.identifier), $.type_annotation, optional(';'))),
    import_statement: $ => seq('import', binding($), 'from', field('module', $.string)),
    try_statement: $ => seq(
      'try', field('body', alias(optional($._block), $.block)),
      'except', field('error', $.identifier), 'then', field('handler', alias(optional($._block), $.block)),
      optional(seq('else', field('otherwise', alias(optional($._block), $.block)))), 'end'
    ),
    export_declaration: $ => prec.right(seq('export', ':', $.type, optional(';'))),
    _att_name_list: $ => list(binding($)),
    _parameter_list: $ => choice(
      seq(list($.parameter), optional(seq(',', $.variadic_parameter))),
      $.variadic_parameter
    ),
    parameter: $ => binding($),
    variadic_parameter: $ => seq('...', optional($.type_annotation)),
    _function_body: $ => seq(
      field('parameters', $.parameters), optional($.return_annotation),
      field('body', alias(optional($._block), $.block)), 'end'
    ),
    type_annotation: $ => seq(':', $.type),
    return_annotation: $ => seq(':', $.return_type),
    return_type: $ => choice($.type, seq('(', optional(list(choice($.type, $.variadic_type))), ')')),
    variadic_type: $ => seq('...', $.type),
    type: $ => choice($.primitive_type, $.named_type, $.optional_type, $.table_type, $.function_type, $.parenthesized_type),
    parenthesized_type: $ => seq('(', $.type, ')'),
    named_type: $ => seq($.identifier, repeat(seq('.', $.identifier))),
    primitive_type: _ => choice('number', 'string', 'boolean', 'nil', 'any'),
    optional_type: $ => prec.left(3, seq($.type, '?')),
    function_type: $ => prec.right(1, seq('function', '(', optional(list(choice($.type, $.variadic_type))), ')', ':', $.return_type)),
    table_type: $ => seq('{', optional(choice($.type, seq(list($.type_field), optional(',')))), '}'),
    type_field: $ => choice(
      seq(field('name', $.identifier), ':', $.type),
      seq('[', $.type, ']', ':', $.type)
    ),
  },
});
