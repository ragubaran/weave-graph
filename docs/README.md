# Documentation map

The current internal documentation has four primary entry points:

| Question | Document |
| --- | --- |
| What is approved? | [plan.md](plan.md) — product/architecture decisions and profile boundaries. |
| What work is complete or planned? | [impl.md](impl.md) — development task list, dependencies and validation status. |
| What is still wrong or undecided? | [issues.md](issues.md) — one issue register, including closed historical IDs and preserved Phase 3 rationale. |
| What is only a draft? | [proposal.md](proposal.md) — unapproved designs and experiments. |

[performance_compare.md](performance_compare.md) remains a **separate,
unchanged** historical measurement record. It is not a current release
benchmark. [codex_review.md](codex_review.md) is the single consolidated
dated Codex audit, including the individual-change status ledger;
[audit_test.md](audit_test.md), [audit-history.md](audit-history.md) and
[missed_metric_issue.md](missed_metric_issue.md) retain test and root-cause
evidence, not an independent current issue list.

The remaining root documents are specialized reference or guide material,
not competing status trackers:

- [beginners-guide.md](beginners-guide.md),
  [deploy.md](deploy.md), [release.md](release.md), and
  [federation-migration-guide.md](federation-migration-guide.md) are guides
  or release specifications; validate commands and external service state
  before using old examples.
- [feature_matrix.md](feature_matrix.md), [user_intent.md](user_intent.md),
  and [slm-spec.md](slm-spec.md) are historical capability/design references.
  Where they differ from the approved Phase 4 profiles, `plan.md` wins.
- [paper.md](paper.md) is research context, not product proof.
- [product/](product/) is the user-facing documentation set.

The older `codex_review_perf.md`, `codex_reviewv2.md`,
`phase3_issues.md`, `proposal_phase4.md`, and `propose.md` files were
consolidated into the primary documents. Speculative `SLLaM.md`,
`comparision.md`, `deep_dive.md`, and duplicate `product_compare.md`
were retired because their unsupported metrics or superseded architecture
could be mistaken for current product facts.
