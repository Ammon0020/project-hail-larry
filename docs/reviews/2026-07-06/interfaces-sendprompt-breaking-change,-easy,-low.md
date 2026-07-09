# SendPrompt signature change is a breaking interface change — verify all implementers/callers updated

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/interfaces/interfaces.go`
- **Lines:** 218

## Description

SendPrompt gained a third parameter `attachments []Attachment`. This is a source-breaking change for any implementer of ACPClient. The concrete implementer (acp.Client.SendPrompt, acp.go:355) and the transportLike interface (acp.go:90) are updated, and all in-tree call sites pass `nil` or a real slice (api.go:524, acp_test.go:135/164, integration_test.go:93/226/232/299/388). go build ./... passes (verified). So this is currently consistent. The finding is low urgency because it's verified clean today, but it's worth flagging: there is no mock ACPClient in the server package (server_test.go:165 only constructs a real *acp.Client via Deps), so a future mock that implements interfaces.ACPClient would need to track this signature. The breaking change is fine for an internal interface but should be called out in docs/STATUS.md or a changelog.

## Recommendation

No code change required (build is green). Consider adding a one-line note in docs/STATUS.md under the image-upload row that SendPrompt's signature changed (breaking for any out-of-tree ACPClient implementer). If a mock ACPClient is added later, ensure it includes the attachments parameter.

## Verification

Ran `go build ./...` — exit 0, no errors. grep'd all SendPrompt( call sites in internal/: api.go:524, acp.go:355, acp_test.go:135/164, integration_test.go:93/226/232/299/388 — all pass either nil or a []interfaces.Attachment. transportLike.Prompt (acp.go:90) also updated to match.
