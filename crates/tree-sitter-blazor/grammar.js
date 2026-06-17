module.exports = grammar({
  name: "blazor",

  extras: ($) => [/\s+/, $.comment],

  rules: {
    source_file: ($) => repeat($._node),

    _node: ($) =>
      choice(
        $.element,
        $.self_closing_element,
        $.code_block,
        $.directive,
        $.inline_expression,
        $.text,
      ),

    element: ($) =>
      seq(
        "<",
        field("tag_name", $.tag_name),
        repeat($.attribute),
        ">",
        repeat($._node),
        "</",
        field("closing_tag_name", $.tag_name),
        ">",
      ),

    self_closing_element: ($) =>
      seq("<", field("tag_name", $.tag_name), repeat($.attribute), "/>"),

    attribute: ($) =>
      seq(
        field("name", $.attribute_name),
        optional(seq("=", field("value", $.attribute_value))),
      ),

    attribute_name: ($) => /[A-Za-z_:][A-Za-z0-9_:\-]*/,

    attribute_value: ($) =>
      choice($.quoted_attribute_value, $.unquoted_attribute_value),

    quoted_attribute_value: ($) =>
      choice(seq('"', /[^\"\n]*/, '"'), seq("'", /[^'\n]*/, "'")),

    unquoted_attribute_value: ($) => /[^\s>]+/,

    tag_name: ($) => /[A-Za-z][A-Za-z0-9\-:]*/,

    directive: ($) =>
      seq("@", field("name", $.identifier), optional($.directive_body)),

    directive_body: ($) => /[^\n]*/,

    code_block: ($) => seq("@code", $.block_expression),

    inline_expression: ($) =>
      seq(
        "@",
        choice($.member_access, $.parenthesized_expression, $.block_expression),
      ),

    member_access: ($) => seq($.identifier, repeat1(seq(".", $.identifier))),

    parenthesized_expression: ($) => seq("(", optional($.csharp_content), ")"),

    block_expression: ($) => seq("{", optional($.csharp_content), "}"),

    csharp_content: ($) => /[^}]+/,

    identifier: ($) => /[A-Za-z_][A-Za-z0-9_]*/,

    text: ($) => /[^<@\n][^<@]*/,

    comment: ($) => choice($.html_comment, $.razor_comment),

    html_comment: ($) => /<!--[^-]*-?->/,

    razor_comment: ($) => /@\*[^*]*\*\@/,
  },
});
