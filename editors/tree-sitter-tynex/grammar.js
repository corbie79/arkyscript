// Editor grammar only. Runtime validity and typing belong to the Rust compiler.
// Extend a pinned, unmodified upstream Lua grammar (see vendor/lua/README.md).
import lua from './vendor/lua/grammar.js';
const list = rule => seq(rule, repeat(seq(',', rule)));
const binding = $ => seq(field('name', $.identifier), optional($.type_annotation));
export default grammar(lua, {
  name: 'tynex',
  rules: {
    identifier: _ => /[A-Za-z_][A-Za-z0-9_]*/,
    statement: ($, original) => choice(original, $.import_statement, $.try_statement),
    declaration: ($, original) => choice(original, $.export_declaration, $.declare_statement),
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
    type: $ => choice($.primitive_type, $.optional_type, $.table_type, $.function_type),
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
