import { StreamLanguage, type StreamParser } from '@codemirror/language'

const parser: StreamParser<null> = {
  startState: () => null,
  token(stream) {
    if (stream.eatSpace()) return null
    if (stream.match('//')) {
      stream.skipToEnd()
      return 'comment'
    }
    if (stream.match(/^"(?:[^"\\]|\\.)*"/)) return 'string'
    if (stream.match(/^%[\w.$-]+/)) return 'variableName'
    if (stream.match(/^@[\w.$-]+/)) return 'labelName'
    if (stream.match(/^!?#?[A-Za-z][\w.$<>?x-]*/)) return 'typeName'
    if (stream.match(/^-?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/)) return 'number'
    if (stream.match(/^[A-Za-z_][\w-]*(?:\.[A-Za-z_][\w-]*)+/)) return 'keyword'
    stream.next()
    return null
  },
}

export const mlirLanguage = StreamLanguage.define(parser)
