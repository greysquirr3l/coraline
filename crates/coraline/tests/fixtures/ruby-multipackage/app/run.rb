# frozen_string_literal: true

require_relative '../lib/helper'

# Runs the app.
class Runner
  def run
    helper = Helper.new
    helper.format(2.0)
  end
end
