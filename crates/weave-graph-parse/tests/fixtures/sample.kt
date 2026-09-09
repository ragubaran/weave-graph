import kotlin.collections.List

interface Named {
    fun getName(): String
}

class Greeter(val name: String) : Named {
    override fun getName(): String {
        return this.name
    }

    fun greet(): String {
        return formatName(this.getName())
    }
}
