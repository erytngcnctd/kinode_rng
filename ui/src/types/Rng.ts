export interface RngMessage {
  rng_source: string
  msg_source: string
  range: { min: number, max: number }
  value: number
  context?: string
  timestamp: Date
}

export interface GetRandom {
  target: string
  context?: string
  range: { min: number, max: number}
}

