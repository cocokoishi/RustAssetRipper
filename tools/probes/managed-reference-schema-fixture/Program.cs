using System;

namespace UnityEngine
{
    public class Object { }
    public class MonoBehaviour : Object { }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class SerializeReference : Attribute { }
}

namespace Game
{
    [Serializable]
    public abstract class GraphNode
    {
        public int value;

        [UnityEngine.SerializeReference]
        public GraphNode? next;

        public UnityEngine.Object? target;
    }

    [Serializable]
    public sealed class NodeA : GraphNode
    {
        public float weight;
    }

    [Serializable]
    public sealed class NodeB : GraphNode
    {
        public string label = string.Empty;
    }

    public sealed class GraphHost : UnityEngine.MonoBehaviour
    {
        [UnityEngine.SerializeReference]
        public GraphNode? root;

        public int marker;
    }
}
