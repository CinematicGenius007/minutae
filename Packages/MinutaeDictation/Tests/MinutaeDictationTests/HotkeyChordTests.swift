import Foundation
import Testing

@testable import MinutaeDictation

@Test func defaultChordIsHoldOptionSpace() {
    #expect(HotkeyRegistration.default.mode == .hold)
    #expect(HotkeyRegistration.default.chord == .optionSpace)
    #expect(HotkeyChord.optionSpace.keyCode == 49)
    #expect(HotkeyChord.optionSpace.modifiers == .option)
    #expect(HotkeyChord.spaceKeyCode == 49)
}

@Test func optionSpaceAndModifiedChordsAreValid() {
    #expect(HotkeyChordValidation.problem(with: .optionSpace) == nil)
    let commandShift = HotkeyChord(keyCode: 40, modifiers: [.command, .shift])
    #expect(HotkeyChordValidation.problem(with: commandShift) == nil)
    let everything = HotkeyChord(
        keyCode: 0,
        modifiers: [.command, .option, .shift, .control]
    )
    #expect(HotkeyChordValidation.problem(with: everything) == nil)
    #expect(
        HotkeyChordValidation.problem(with: HotkeyChord(keyCode: 0x7E, modifiers: .control)) == nil)
}

@Test func chordRejectsABareKeyAModifierKeyAndAnOutOfRangeCode() {
    #expect(
        HotkeyChordValidation.problem(with: HotkeyChord(keyCode: 49, modifiers: []))
            == .missingModifier
    )
    #expect(
        HotkeyChordValidation.problem(with: HotkeyChord(keyCode: 0x3A, modifiers: .option))
            == .keyIsModifier
    )
    #expect(
        HotkeyChordValidation.problem(with: HotkeyChord(keyCode: 0x3F, modifiers: .command))
            == .keyIsModifier
    )
    #expect(
        HotkeyChordValidation.problem(with: HotkeyChord(keyCode: 127, modifiers: .option))
            == .keyCodeOutOfRange
    )
}

@Test func optionOnlyFailureAddsControlOnTheSameKey() {
    #expect(HotkeyFallback.suggestion(after: .optionSpace) == HotkeyFallback.controlOptionSpace)
    let optionK = HotkeyChord(keyCode: 0x28, modifiers: .option)
    #expect(
        HotkeyFallback.suggestion(after: optionK)
            == HotkeyChord(keyCode: 0x28, modifiers: [.control, .option])
    )
}

@Test func otherFailuresWalkTheCandidateList() {
    #expect(
        HotkeyFallback.suggestion(after: HotkeyFallback.controlOptionSpace) == .optionSpace
    )
    let commandK = HotkeyChord(keyCode: 40, modifiers: .command)
    #expect(HotkeyFallback.suggestion(after: commandK) == .optionSpace)
    let bare = HotkeyChord(keyCode: 49, modifiers: [])
    #expect(HotkeyFallback.suggestion(after: bare) == .optionSpace)
}

@Test func suggestionIsAlwaysADifferentValidChord() {
    var samples = HotkeyFallback.candidates
    samples.append(HotkeyChord(keyCode: 0x28, modifiers: .option))
    samples.append(HotkeyChord(keyCode: 49, modifiers: []))
    samples.append(HotkeyChord(keyCode: 200, modifiers: .command))
    samples.append(HotkeyChord(keyCode: 0x37, modifiers: .option))
    for failed in samples {
        let suggestion = HotkeyFallback.suggestion(after: failed)
        #expect(suggestion != failed)
        #expect(HotkeyChordValidation.problem(with: suggestion) == nil)
    }
}

@Test func chordRoundTripsAsKeyCodeAndModifiers() throws {
    let chord = HotkeyChord(keyCode: 49, modifiers: [.option, .control])
    let data = try JSONEncoder().encode(chord)
    let decoded = try JSONDecoder().decode(HotkeyChord.self, from: data)
    #expect(decoded == chord)
    let json = String(decoding: data, as: UTF8.self)
    #expect(json.contains("\"keyCode\":49"))
    #expect(json.contains("\"modifiers\":"))
}
