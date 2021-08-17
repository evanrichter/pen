Feature: String
  Background:
    Given a file named "pen.json" with:
    """json
    {
      "dependencies": {
        "System": "file://pen-root/lib/os"
      }
    }
    """

  Scenario: Compare strings
    Given a file named "Main.pen" with:
    """pen
    import System'Os

    main = \(os Os'Os) number {
      if "foo" == "foo" {
        0
      } else {
        1
      }
    }
    """
    When I successfully run `pen build`
    Then I successfully run `./app`