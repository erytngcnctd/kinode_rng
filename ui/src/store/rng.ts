import { create } from 'zustand'
import { RngMessage } from '../types/Rng'
import { persist } from 'zustand/middleware'

export interface RandomsStore {
  randoms: RngMessage[],
  newRandom: (random: RngMessage) => void
  theme: String,
  changeTheme: () => void
  get: () => RandomsStore,
  set: (partial: RandomsStore | Partial<RandomsStore>) => void
}

const useRandomStore = create<RandomsStore>()(
  persist(
    (set, get) => ({
      randoms: [],
      newRandom: (data: RngMessage) => {
        const state = get()
        const { rng_source, msg_source, range, value, context, timestamp } = data
        state.randoms.unshift({ rng_source, msg_source, range, value, context, timestamp })
        set( state )
      },
    theme: JSON.parse(localStorage.getItem("kinode_rng"))?.state.theme || 'light',
    changeTheme: () => {
      const state = get()
      const newTheme = state.theme === 'light' ? 'dark' : 'light'
      set({...state, theme: newTheme}) 
    },
    get,
    set,
  }),
    {
      name: 'kinode_rng', 
    }
  )
)

export default useRandomStore
