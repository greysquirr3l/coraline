module geometry
  use iso_fortran_env, only: real64
  implicit none
  public :: area
  private

  type :: shape_base
  end type shape_base

  type, extends(shape_base) :: circle
    real(real64) :: radius
  end type circle

contains

  function area(c) result(a)
    type(circle) :: c
    real(real64) :: a
    a = 3.14159 * c%radius * c%radius
    call report(a)
  end function area

  subroutine report(a)
    real(real64) :: a
    print *, a
  end subroutine report

end module geometry
