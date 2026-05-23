import Testing
@testable import PlaneAICore

@Suite("FuzzyMatch")
struct FuzzyMatchTests {
    @Test func emptyQueryMatchesEverything() {
        #expect(fuzzyMatch(query: "", target: "anything") != nil)
    }

    @Test func exactSubstringMatches() {
        #expect(fuzzyMatch(query: "fix", target: "fix-auth-bug") != nil)
    }

    @Test func substringInMiddleMatches() {
        #expect(fuzzyMatch(query: "auth", target: "fix-auth-bug") != nil)
    }

    @Test func noMatchReturnsNil() {
        #expect(fuzzyMatch(query: "xyz", target: "fix-auth-bug") == nil)
    }

    @Test func caseInsensitive() {
        #expect(fuzzyMatch(query: "FIX", target: "fix-auth-bug") != nil)
    }

    @Test func prefixMatchScoresHigher() {
        let prefix = fuzzyMatch(query: "fix", target: "fix-auth-bug")!
        let middle = fuzzyMatch(query: "aut", target: "fix-auth-bug")!
        #expect(prefix > middle)
    }

    @Test func subsequenceMatches() {
        // "fab" = f(ix)-a(uth)-b(ug) — non-contiguous but valid subsequence
        let result = fuzzyMatch(query: "fab", target: "fix-auth-bug")
        #expect(result != nil)
    }

    @Test func wordBoundaryScoresHigher() {
        let boundary = fuzzyMatch(query: "auth", target: "fix-auth-bug")!
        let midWord = fuzzyMatch(query: "auth", target: "fixauthbug")!
        #expect(boundary > midWord)
    }

    @Test func consecutiveCharsScoreHigher() {
        let consecutive = fuzzyMatch(query: "fix", target: "fix-auth-bug")!
        let scattered = fuzzyMatch(query: "fab", target: "fix-auth-bug")!
        #expect(consecutive > scattered)
    }

    @Test func acronymMatching() {
        // Matches first letter of each word
        let result = fuzzyMatch(query: "ns", target: "new-session")
        #expect(result != nil)
    }

    @Test func shorterTargetScoresHigher() {
        let short = fuzzyMatch(query: "fix", target: "fix-bug")!
        let long = fuzzyMatch(query: "fix", target: "fix-authentication-bug-in-oauth")!
        #expect(short > long)
    }

    @Test func queryLongerThanTargetReturnsNil() {
        #expect(fuzzyMatch(query: "longquery", target: "short") == nil)
    }
}
