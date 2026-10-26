package Greeter;

use strict;
use warnings;
use base 'Exporter';

our @EXPORT_OK = qw(greet farewell);

sub greet {
    my ($name) = @_;
    return "hello, $name";
}

sub farewell {
    my ($name) = @_;
    return "bye, $name";
}

1;
