import Testing

@testable import MinutaeLibrary

@Test func matchExpressionQuotesTokensAndPrefixesTheLast() {
    #expect(FTSQuery.matchExpression(for: "apollo launch") == "\"apollo\" \"launch\"*")
    #expect(FTSQuery.matchExpression(for: "  budget  ") == "\"budget\"*")
    #expect(FTSQuery.matchExpression(for: "budget***") == "\"budget\"*")
    #expect(FTSQuery.matchExpression(for: "\"apollo\"") == "\"apollo\"*")
}

@Test func matchExpressionNeutralisesOperatorsAndSyntax() {
    #expect(FTSQuery.matchExpression(for: "AND OR NEAR") == "\"AND\" \"OR\" \"NEAR\"*")
    #expect(FTSQuery.matchExpression(for: "apollo AND launch") == "\"apollo\" \"AND\" \"launch\"*")
    #expect(
        FTSQuery.matchExpression(for: "NEAR(apollo, launch)") == "\"NEAR\" \"apollo\" \"launch\"*")
    #expect(FTSQuery.matchExpression(for: "((((apollo") == "\"apollo\"*")
    #expect(FTSQuery.matchExpression(for: "apollo)))") == "\"apollo\"*")
    #expect(FTSQuery.matchExpression(for: "(apollo OR launch)") == "\"apollo\" \"OR\" \"launch\"*")
    #expect(FTSQuery.matchExpression(for: "raw_text:apollo") == "\"raw\" \"text\" \"apollo\"*")
    #expect(
        FTSQuery.matchExpression(for: "'; DROP TABLE meetings; --")
            == "\"DROP\" \"TABLE\" \"meetings\"*"
    )
}

@Test func matchExpressionDropsEmptyAndStarQueries() {
    #expect(FTSQuery.matchExpression(for: "") == nil)
    #expect(FTSQuery.matchExpression(for: "   ") == nil)
    #expect(FTSQuery.matchExpression(for: "*") == nil)
    #expect(FTSQuery.matchExpression(for: "\"\"") == nil)
    #expect(FTSQuery.matchExpression(for: "(((((") == nil)
}

@Test func quotedTokenDoublesInternalQuotes() {
    #expect(FTSQuery.quotedToken("a\"b", prefix: false) == "\"a\"\"b\"")
    #expect(FTSQuery.quotedToken("a\"b", prefix: true) == "\"a\"\"b\"*")
}

@Test func keywordSnippetMarksExactAndPrefixTokens() {
    let invoice = KeywordSnippet.make(
        text: "Send the invoice number 4412 to the vendor.",
        tokens: ["invoice", "4412"]
    )
    #expect(invoice.contains("<mark>invoice</mark>"))
    #expect(invoice.contains("<mark>4412</mark>"))
    let prefix = KeywordSnippet.make(text: "The apollo launch window.", tokens: ["apoll"])
    #expect(prefix.contains("<mark>apollo</mark>"))
    #expect(!prefix.contains("<mark>The</mark>"))
}
