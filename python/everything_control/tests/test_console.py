from everything_control.console import build_parser


def test_help_parser_builds() -> None:
    parser = build_parser()
    help_text = parser.format_help()
    assert "everything-control" in help_text
    assert "--base-url" in help_text


def test_parser_exposes_research_impact_and_github_commands() -> None:
    parser = build_parser()
    args = parser.parse_args(["research-search", "latest API", "--freshness", "week"])
    assert args.command == "research-search"
    assert args.freshness == "week"
    args = parser.parse_args(["connector", "github"])
    assert args.provider == "github"
    args = parser.parse_args(["graph-change-impact", "impact.json"])
    assert args.request.name == "impact.json"
