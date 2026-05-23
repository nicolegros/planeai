import Foundation

/// Fuzzy-matches `query` against `target` using subsequence matching.
/// Returns a score > 0 on match, nil on no match.
/// Scoring: consecutive bonus, prefix bonus, word-boundary bonus, gap penalty.
public func fuzzyMatch(query: String, target: String) -> Int? {
    if query.isEmpty { return 1 }

    let queryChars = Array(query.lowercased())
    let targetChars = Array(target.lowercased())

    guard queryChars.count <= targetChars.count else { return nil }

    // Find best match using greedy-with-backtrack for scoring
    var bestScore: Int?
    findBestMatch(queryChars: queryChars, targetChars: targetChars, bestScore: &bestScore)
    return bestScore
}

private func findBestMatch(queryChars: [Character], targetChars: [Character], bestScore: inout Int?) {
    // Try all valid starting positions for the first character
    for startIdx in targetChars.indices {
        guard targetChars[startIdx] == queryChars[0] else { continue }
        // Greedily match remaining characters from this start
        if let score = scoreMatch(queryChars: queryChars, targetChars: targetChars, startAt: startIdx) {
            if bestScore == nil || score > bestScore! {
                bestScore = score
            }
        }
    }
}

private func scoreMatch(queryChars: [Character], targetChars: [Character], startAt: Int) -> Int? {
    var matchPositions: [Int] = []
    matchPositions.reserveCapacity(queryChars.count)

    var qi = 0
    var ti = startAt

    while qi < queryChars.count && ti < targetChars.count {
        if targetChars[ti] == queryChars[qi] {
            matchPositions.append(ti)
            qi += 1
        }
        ti += 1
    }

    guard qi == queryChars.count else { return nil }

    // Score the match
    var score = 0
    let matchCount = matchPositions.count

    for (i, pos) in matchPositions.enumerated() {
        // Base: each matched char
        score += 10

        // Prefix bonus
        if pos == i {
            score += 8
        }

        // Consecutive bonus
        if i > 0 && matchPositions[i] == matchPositions[i - 1] + 1 {
            score += 6
        }

        // Word-boundary bonus (char after separator or camelCase)
        if pos > 0 {
            let prev = targetChars[pos - 1]
            if prev == "-" || prev == "_" || prev == " " || prev == "/" || prev == "." {
                score += 7
            } else if prev.isLowercase && targetChars[pos].isUppercase {
                score += 7
            }
        } else {
            // First character of target
            score += 8
        }

        // Gap penalty
        if i > 0 {
            let gap = matchPositions[i] - matchPositions[i - 1] - 1
            score -= gap * 2
        }
    }

    // Shorter targets score higher (tighter match)
    score += max(0, 50 - targetChars.count)

    // Bonus for matching a larger fraction of the target
    score += (matchCount * 10) / max(targetChars.count, 1)

    return score
}
