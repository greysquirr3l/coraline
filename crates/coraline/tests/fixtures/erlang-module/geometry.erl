-module(geometry).
-export([area/1, total/1]).

-record(circle, {radius = 0.0}).

area(#circle{radius = Radius}) ->
    3.14159 * Radius * Radius.

total(Shapes) ->
    lists:sum(Shapes).
